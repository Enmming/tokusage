"""One-time auth state helpers for Enterprise WeChat login."""

from __future__ import annotations

import datetime as dt
import secrets

from fastapi import HTTPException
from sqlalchemy.ext.asyncio import AsyncSession

from ..models import AuthFlowState
from ..security import hash_secret


STATE_TTL_MINUTES = 10


def now_utc() -> dt.datetime:
    return dt.datetime.now(dt.timezone.utc)


def _as_aware_utc(value: dt.datetime) -> dt.datetime:
    if value.tzinfo is None:
        return value.replace(tzinfo=dt.timezone.utc)
    return value.astimezone(dt.timezone.utc)


def normalize_return_to(return_to: str | None) -> str:
    if not return_to:
        return "/dashboard"
    path = return_to.split("?", 1)[0].split("#", 1)[0]
    if (
        not return_to.startswith("/")
        or return_to.startswith("//")
        or "\\" in return_to
        or any(ord(char) < 32 or ord(char) == 127 for char in return_to)
        or not (path == "/dashboard" or path.startswith("/dashboard/"))
    ):
        raise HTTPException(status_code=400, detail="Invalid return_to")
    return return_to


async def create_state(
    session: AsyncSession,
    *,
    provider: str,
    entry: str,
    return_to: str | None,
) -> str:
    state = secrets.token_urlsafe(32)
    session.add(
        AuthFlowState(
            state_hash=hash_secret(state),
            provider=provider,
            entry=entry,
            return_to=normalize_return_to(return_to),
            expires_at=now_utc() + dt.timedelta(minutes=STATE_TTL_MINUTES),
        )
    )
    await session.commit()
    return state


async def consume_state(session: AsyncSession, state: str) -> AuthFlowState:
    row = await session.get(AuthFlowState, hash_secret(state), with_for_update=True)
    current_time = now_utc()
    if (
        row is None
        or row.consumed_at is not None
        or _as_aware_utc(row.expires_at) <= current_time
    ):
        raise HTTPException(status_code=400, detail="Invalid or expired state")

    row.consumed_at = current_time
    await session.commit()
    return row
