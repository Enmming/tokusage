"""Portal session cookie creation and validation."""

from __future__ import annotations

import datetime as dt
import secrets

from sqlalchemy.ext.asyncio import AsyncSession

from ..config import settings
from ..models import PortalSession, PortalUser
from ..security import hash_secret, sign_value, unsign_value


SESSION_COOKIE_NAME = "tokusage_session"


def now_utc() -> dt.datetime:
    return dt.datetime.now(dt.timezone.utc)


def _as_aware_utc(value: dt.datetime) -> dt.datetime:
    if value.tzinfo is None:
        return value.replace(tzinfo=dt.timezone.utc)
    return value.astimezone(dt.timezone.utc)


async def create_session(session: AsyncSession, user: PortalUser) -> str:
    raw_secret = secrets.token_urlsafe(32)
    expires_at = now_utc() + dt.timedelta(days=settings.portal_session_days)
    session.add(
        PortalSession(
            session_hash=hash_secret(raw_secret),
            user_id=user.id,
            expires_at=expires_at,
        )
    )
    await session.commit()
    return sign_value(raw_secret, settings.portal_session_secret)


async def load_user_from_signed_session(
    session: AsyncSession,
    signed_value: str | None,
) -> PortalUser | None:
    if not signed_value:
        return None
    raw_secret = unsign_value(signed_value, settings.portal_session_secret)
    if raw_secret is None:
        return None

    row = await session.get(PortalSession, hash_secret(raw_secret))
    if row is None or row.revoked_at is not None:
        return None
    if _as_aware_utc(row.expires_at) <= now_utc():
        return None

    user = await session.get(PortalUser, row.user_id)
    if user is None or user.status != "active":
        return None

    row.last_seen_at = now_utc()
    await session.commit()
    return user


async def revoke_session(session: AsyncSession, signed_value: str | None) -> None:
    if not signed_value:
        return
    raw_secret = unsign_value(signed_value, settings.portal_session_secret)
    if raw_secret is None:
        return
    row = await session.get(PortalSession, hash_secret(raw_secret))
    if row is None or row.revoked_at is not None:
        return
    row.revoked_at = now_utc()
    await session.commit()
