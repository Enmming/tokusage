"""HTTP routes."""

from fastapi import APIRouter, Depends
from sqlalchemy import case, func, select
from sqlalchemy.dialects.postgresql import insert as pg_insert
from sqlalchemy.dialects.sqlite import insert as sqlite_insert
from sqlalchemy.ext.asyncio import AsyncSession

from .auth import require_bearer_token
from .db import get_session
from .models import DailyUsage, Submission
from .schemas import SubmitPayload, SubmitResponse

router = APIRouter()


def _greatest(a, b):
    """Portable GREATEST(a, b). Postgres has GREATEST; SQLite doesn't —
    CASE works on both."""
    return case((a > b, a), else_=b)


@router.get("/health")
async def health() -> dict:
    return {"ok": True}


@router.post("/api/submit", response_model=SubmitResponse)
async def submit(
    payload: SubmitPayload,
    token: str = Depends(require_bearer_token),
    session: AsyncSession = Depends(get_session),
) -> SubmitResponse:
    host_id = payload.meta.host_id
    client_version = payload.meta.client_version

    # 1) Ledger row for the raw submission (audit trail).
    total_tokens = sum(
        c.tokens.input
        + c.tokens.output
        + c.tokens.cache_read
        + c.tokens.cache_write
        + c.tokens.reasoning
        for c in payload.contributions
    )
    total_cost = sum(c.cost_cents for c in payload.contributions)
    submission = Submission(
        host_id=host_id,
        client_version=client_version,
        contribution_count=len(payload.contributions),
        total_tokens=total_tokens,
        total_cost_cents=total_cost,
    )
    session.add(submission)
    await session.flush()

    # 2) UPSERT each contribution into the canonical daily_usage table.
    #    On conflict (host_id, date, client, model, provider) take
    #    GREATEST(existing, incoming) so partial or stale submissions never
    #    shrink previously-captured totals.
    dialect = session.bind.dialect.name if session.bind else "postgresql"
    _insert = pg_insert if dialect == "postgresql" else sqlite_insert

    upserted = 0
    for c in payload.contributions:
        stmt = _insert(DailyUsage).values(
            host_id=host_id,
            date=c.date,
            client=c.client,
            model=c.model,
            provider=c.provider,
            input_tokens=c.tokens.input,
            output_tokens=c.tokens.output,
            cache_read_tokens=c.tokens.cache_read,
            cache_write_tokens=c.tokens.cache_write,
            reasoning_tokens=c.tokens.reasoning,
            cost_cents=c.cost_cents,
            message_count=c.message_count,
        )
        set_ = {
            "input_tokens": _greatest(
                DailyUsage.input_tokens, stmt.excluded.input_tokens
            ),
            "output_tokens": _greatest(
                DailyUsage.output_tokens, stmt.excluded.output_tokens
            ),
            "cache_read_tokens": _greatest(
                DailyUsage.cache_read_tokens, stmt.excluded.cache_read_tokens
            ),
            "cache_write_tokens": _greatest(
                DailyUsage.cache_write_tokens, stmt.excluded.cache_write_tokens
            ),
            "reasoning_tokens": _greatest(
                DailyUsage.reasoning_tokens, stmt.excluded.reasoning_tokens
            ),
            "cost_cents": _greatest(
                DailyUsage.cost_cents, stmt.excluded.cost_cents
            ),
            "message_count": _greatest(
                DailyUsage.message_count, stmt.excluded.message_count
            ),
            "last_updated_at": func.now(),
        }
        if dialect == "postgresql":
            stmt = stmt.on_conflict_do_update(constraint="uq_daily_usage", set_=set_)
        else:
            stmt = stmt.on_conflict_do_update(
                index_elements=["host_id", "date", "client", "model", "provider"],
                set_=set_,
            )
        await session.execute(stmt)
        upserted += 1

    await session.commit()
    return SubmitResponse(
        submission_id=submission.id,
        contributions_upserted=upserted,
    )


@router.get("/api/summary")
async def summary(
    _token: str = Depends(require_bearer_token),
    session: AsyncSession = Depends(get_session),
) -> dict:
    """Rough sanity endpoint — totals across all rows. Real dashboards live
    in Metabase/Grafana."""
    stmt = select(
        func.count(DailyUsage.id).label("rows"),
        func.coalesce(func.sum(DailyUsage.input_tokens), 0),
        func.coalesce(func.sum(DailyUsage.output_tokens), 0),
        func.coalesce(func.sum(DailyUsage.cost_cents), 0.0),
        func.count(func.distinct(DailyUsage.host_id)),
    )
    row = (await session.execute(stmt)).one()
    return {
        "rows": row[0],
        "total_input_tokens": row[1],
        "total_output_tokens": row[2],
        "total_cost_cents": round(float(row[3]), 2),
        "unique_hosts": row[4],
    }
