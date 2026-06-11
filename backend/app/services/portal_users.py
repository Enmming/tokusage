"""Portal user and personal API token lifecycle."""

from __future__ import annotations

import datetime as dt
from typing import Any

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from ..models import PortalUser, UserToken
from ..security import generate_api_token, hash_token, token_hint


def now_utc() -> dt.datetime:
    return dt.datetime.now(dt.timezone.utc)


def derive_secondary_department(path: list[str] | None) -> str:
    if not path:
        return ""
    if len(path) == 1:
        return path[0]
    return path[1]


def _department_path(profile: dict[str, Any]) -> list[str]:
    raw_path = profile.get("department_path") or []
    if isinstance(raw_path, str):
        return [part for part in raw_path.split("/") if part]
    if isinstance(raw_path, list):
        return [str(part) for part in raw_path if str(part)]
    return []


def _display_name(userid: str, profile: dict[str, Any]) -> str:
    return str(profile.get("name") or f"企业微信用户 {userid}")


async def login_wecom_user(
    session: AsyncSession,
    *,
    corp_id: str,
    userid: str,
    profile: dict[str, Any],
) -> PortalUser:
    now = now_utc()
    path = _department_path(profile)
    secondary_department = derive_secondary_department(path)
    name = _display_name(userid, profile)
    avatar_url = profile.get("avatar_url") or profile.get("avatar")

    user = (
        await session.execute(
            select(PortalUser)
            .where(PortalUser.wecom_corp_id == corp_id)
            .where(PortalUser.wecom_userid == userid)
            .with_for_update()
        )
    ).scalar_one_or_none()

    if user is None:
        user = PortalUser(
            wecom_corp_id=corp_id,
            wecom_userid=userid,
            name=name,
            avatar_url=avatar_url,
            department_path_json=path,
            secondary_department=secondary_department,
            status="active",
            last_login_at=now,
        )
        session.add(user)
        await session.flush()
    else:
        user.name = name
        user.avatar_url = avatar_url
        user.department_path_json = path
        user.secondary_department = secondary_department
        user.last_login_at = now

    await ensure_active_token(session, user)
    await session.commit()
    await session.refresh(user)
    return user


async def ensure_active_token(session: AsyncSession, user: PortalUser) -> UserToken:
    token = (
        await session.execute(
            select(UserToken)
            .where(UserToken.user_id == user.id)
            .where(UserToken.active.is_(True))
            .where(UserToken.revoked_at.is_(None))
            .limit(1)
        )
    ).scalar_one_or_none()
    if token is not None:
        return token

    plain_token = generate_api_token()
    token = UserToken(
        user_id=user.id,
        team_id=user.secondary_department or "unknown",
        user_label=user.name,
        plain_token=plain_token,
        token_hash=hash_token(plain_token),
        token_hint=token_hint(plain_token),
        active=True,
    )
    session.add(token)
    await session.flush()
    return token
