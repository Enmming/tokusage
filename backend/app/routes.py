"""HTTP routes."""

import asyncio
import datetime as dt

from fastapi import APIRouter, Depends, HTTPException, Query, status
from sqlalchemy import select
from sqlalchemy.exc import IntegrityError
from sqlalchemy.ext.asyncio import AsyncSession

from .auth import require_user_token
from .db import get_session
from .models import ConflictEvent, RawUsageEvent, UserToken
from .schemas import SubmitEvent, SubmitPayload, SubmitResponse, SummaryRow
from .security import hash_content
from .summary import fetch_user_summary_rows

router = APIRouter()


@router.get("/health")
async def health() -> dict:
    return {"ok": True}


@router.post("/api/submit", response_model=SubmitResponse)
async def submit(
    payload: SubmitPayload,
    user_token: UserToken = Depends(require_user_token),
    session: AsyncSession = Depends(get_session),
) -> SubmitResponse:
    inserted = 0
    duplicates_ignored = 0
    conflicts_ignored = 0

    for event in payload.events:
        content_hash = hash_content(
            event.model_dump(mode="json", exclude={"session_key", "seq"})
        )
        inserted_row = await try_insert_event(
            session=session,
            user_token=user_token,
            event=event,
            content_hash=content_hash,
            client_version=payload.client_version,
            submitted_at=payload.submitted_at,
        )

        if inserted_row:
            inserted += 1
            continue

        existing, retry_inserted = await resolve_existing_after_conflict(
            session=session,
            user_token=user_token,
            event=event,
            content_hash=content_hash,
            client_version=payload.client_version,
            submitted_at=payload.submitted_at,
        )
        if retry_inserted:
            inserted += 1
            continue
        if existing.content_hash == content_hash:
            duplicates_ignored += 1
            continue

        session.add(
            ConflictEvent(
                user_token_id=user_token.id,
                source=event.source,
                event_key=event.event_key,
                existing_event_id=existing.id,
                existing_content_hash=existing.content_hash,
                incoming_content_hash=content_hash,
                incoming_payload_json=event.model_dump(mode="json"),
                client_version=payload.client_version,
                submitted_at=payload.submitted_at,
                reason="content_mismatch",
            )
        )
        conflicts_ignored += 1

    await session.commit()

    return SubmitResponse(
        received=len(payload.events),
        inserted=inserted,
        duplicates_ignored=duplicates_ignored,
        conflicts_ignored=conflicts_ignored,
    )


@router.get("/api/summary", response_model=list[SummaryRow])
async def summary(
    date_from: dt.date | None = Query(default=None, alias="from"),
    date_to: dt.date | None = Query(default=None, alias="to"),
    source: str | None = Query(default=None),
    model: str | None = Query(default=None),
    provider: str | None = Query(default=None),
    user_token: UserToken = Depends(require_user_token),
    session: AsyncSession = Depends(get_session),
) -> list[SummaryRow]:
    if date_from is not None and date_to is not None and date_from > date_to:
        raise HTTPException(
            status_code=status.HTTP_422_UNPROCESSABLE_ENTITY,
            detail="'from' must be before or equal to 'to'",
        )

    rows = await fetch_user_summary_rows(
        session,
        user_token,
        date_from=date_from,
        date_to=date_to,
        source=source,
        model=model,
        provider=provider,
    )
    await session.commit()
    return rows


async def try_insert_event(
    *,
    session: AsyncSession,
    user_token: UserToken,
    event: SubmitEvent,
    content_hash: str,
    client_version: str,
    submitted_at: dt.datetime,
) -> bool:
    try:
        async with session.begin_nested():
            session.add(
                RawUsageEvent(
                    user_token_id=user_token.id,
                    source=event.source,
                    event_key=event.event_key,
                    event_ts=event.event_ts,
                    session_key=event.session_key,
                    seq=event.seq,
                    model=event.model,
                    provider=event.provider,
                    input_tokens=event.tokens.input,
                    output_tokens=event.tokens.output,
                    cache_read_tokens=event.tokens.cache_read,
                    cache_write_tokens=event.tokens.cache_write,
                    reasoning_tokens=event.tokens.reasoning,
                    cost_cents=event.cost_cents,
                    content_hash=content_hash,
                    raw_payload_json=event.raw_payload,
                    client_version=client_version,
                    submitted_at=submitted_at,
                )
            )
            await session.flush()
        return True
    except IntegrityError:
        return False


async def resolve_existing_after_conflict(
    *,
    session: AsyncSession,
    user_token: UserToken,
    event: SubmitEvent,
    content_hash: str,
    client_version: str,
    submitted_at: dt.datetime,
) -> tuple[RawUsageEvent, bool]:
    for attempt in range(3):
        existing = await find_existing_event(session, user_token, event)
        if existing is not None:
            return existing, False
        await asyncio.sleep(0.01 * (attempt + 1))
        if await try_insert_event(
            session=session,
            user_token=user_token,
            event=event,
            content_hash=content_hash,
            client_version=client_version,
            submitted_at=submitted_at,
        ):
            return await find_existing_event_required(session, user_token, event), True

    return await find_existing_event_required(session, user_token, event), False


async def find_existing_event(
    session: AsyncSession,
    user_token: UserToken,
    event: SubmitEvent,
) -> RawUsageEvent | None:
    existing_stmt = (
        select(RawUsageEvent)
        .where(RawUsageEvent.user_token_id == user_token.id)
        .where(RawUsageEvent.source == event.source)
        .where(RawUsageEvent.event_key == event.event_key)
        .limit(1)
    )
    return (await session.execute(existing_stmt)).scalar_one_or_none()


async def find_existing_event_required(
    session: AsyncSession,
    user_token: UserToken,
    event: SubmitEvent,
) -> RawUsageEvent:
    existing = await find_existing_event(session, user_token, event)
    if existing is None:
        # This should only happen if a competing transaction rolled back after
        # we observed a unique-key conflict. Let the request fail loudly rather
        # than silently dropping data.
        raise RuntimeError("event insert conflicted but no existing row was found")
    return existing
