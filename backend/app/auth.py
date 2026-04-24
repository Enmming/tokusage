"""DB-backed bearer-token auth dependency."""

import datetime as dt

from fastapi import Depends, Header, HTTPException, status
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from .db import get_session
from .models import UserToken
from .security import hash_token


async def require_user_token(
    authorization: str | None = Header(default=None),
    session: AsyncSession = Depends(get_session),
) -> UserToken:
    if not authorization or not authorization.lower().startswith("bearer "):
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="missing Bearer token",
            headers={"WWW-Authenticate": "Bearer"},
        )

    token = authorization.split(" ", 1)[1].strip()
    token_hash = hash_token(token)
    stmt = select(UserToken).where(UserToken.token_hash == token_hash).limit(1)
    user_token = (await session.execute(stmt)).scalar_one_or_none()

    if user_token is None or not user_token.active or user_token.revoked_at is not None:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="invalid token",
            headers={"WWW-Authenticate": "Bearer"},
        )

    user_token.last_used_at = dt.datetime.now(dt.timezone.utc)
    return user_token
