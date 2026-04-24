"""HTTP routes."""

from fastapi import APIRouter, Depends
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from .auth import require_user_token
from .db import get_session
from .models import ConflictEvent, RawUsageEvent, UserToken
from .schemas import SubmitPayload, SubmitResponse
from .security import hash_content

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
        normalized_event = event.model_dump(mode="json")
        content_hash = hash_content(
            event.model_dump(mode="json", exclude={"session_key", "seq"})
        )

        existing_stmt = (
            select(RawUsageEvent)
            .where(RawUsageEvent.user_token_id == user_token.id)
            .where(RawUsageEvent.source == event.source)
            .where(RawUsageEvent.event_key == event.event_key)
            .limit(1)
        )
        existing = (await session.execute(existing_stmt)).scalar_one_or_none()

        if existing is None:
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
                    client_version=payload.client_version,
                    submitted_at=payload.submitted_at,
                )
            )
            await session.flush()
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
                incoming_payload_json=normalized_event,
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
