"""Summary query helpers for authenticated usage reporting."""

from __future__ import annotations

import datetime as dt
from decimal import Decimal
from typing import Any

from sqlalchemy import func, select
from sqlalchemy.ext.asyncio import AsyncSession

from .models import RawUsageEvent, UserToken
from .schemas import SummaryRow


def _normalize_value(value: Any) -> Any:
    if isinstance(value, dt.date):
        return value.isoformat()
    if isinstance(value, Decimal):
        return float(value)
    return value


async def fetch_user_summary_rows(
    session: AsyncSession,
    user_token: UserToken,
    *,
    date_from: dt.date | None = None,
    date_to: dt.date | None = None,
    source: str | None = None,
    model: str | None = None,
    provider: str | None = None,
) -> list[SummaryRow]:
    usage_date = func.date(RawUsageEvent.event_ts).label("usage_date")
    stmt = (
        select(
            usage_date,
            RawUsageEvent.source,
            RawUsageEvent.model,
            RawUsageEvent.provider,
            func.count().label("event_count"),
            func.coalesce(func.sum(RawUsageEvent.input_tokens), 0).label("input_tokens"),
            func.coalesce(func.sum(RawUsageEvent.output_tokens), 0).label(
                "output_tokens"
            ),
            func.coalesce(func.sum(RawUsageEvent.cache_read_tokens), 0).label(
                "cache_read_tokens"
            ),
            func.coalesce(func.sum(RawUsageEvent.cache_write_tokens), 0).label(
                "cache_write_tokens"
            ),
            func.coalesce(func.sum(RawUsageEvent.reasoning_tokens), 0).label(
                "reasoning_tokens"
            ),
            func.coalesce(func.sum(RawUsageEvent.cost_cents), 0).label("cost_cents"),
        )
        .where(RawUsageEvent.user_token_id == user_token.id)
        .group_by(
            usage_date,
            RawUsageEvent.source,
            RawUsageEvent.model,
            RawUsageEvent.provider,
        )
        .order_by(
            usage_date,
            RawUsageEvent.source,
            RawUsageEvent.model,
            RawUsageEvent.provider,
        )
    )

    if date_from is not None:
        stmt = stmt.where(func.date(RawUsageEvent.event_ts) >= date_from.isoformat())
    if date_to is not None:
        stmt = stmt.where(func.date(RawUsageEvent.event_ts) <= date_to.isoformat())
    if source:
        stmt = stmt.where(RawUsageEvent.source == source)
    if model:
        stmt = stmt.where(RawUsageEvent.model == model)
    if provider:
        stmt = stmt.where(RawUsageEvent.provider == provider)

    result = await session.execute(stmt)
    rows = []
    for row in result.mappings().all():
        rows.append(
            SummaryRow(
                team_id=user_token.team_id,
                user_label=user_token.user_label,
                usage_date=str(_normalize_value(row["usage_date"])),
                source=row["source"],
                model=row["model"],
                provider=row["provider"],
                event_count=row["event_count"],
                input_tokens=row["input_tokens"],
                output_tokens=row["output_tokens"],
                cache_read_tokens=row["cache_read_tokens"],
                cache_write_tokens=row["cache_write_tokens"],
                reasoning_tokens=row["reasoning_tokens"],
                cost_cents=float(_normalize_value(row["cost_cents"])),
            )
        )
    return rows
