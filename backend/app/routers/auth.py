"""Portal authentication routes."""

from __future__ import annotations

import inspect

from fastapi import APIRouter, Cookie, Depends, HTTPException, Query, Response
from fastapi.responses import RedirectResponse
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from ..config import settings
from ..db import get_session
from ..models import UserToken
from ..services import auth_flow, portal_sessions, portal_users
from ..services.wecom import WeComClient


router = APIRouter()


@router.get("/api/auth/wecom/login-url")
async def wecom_login_url(
    entry: str = Query(default="qr"),
    return_to: str | None = Query(default=None),
    session: AsyncSession = Depends(get_session),
) -> dict[str, str]:
    client = WeComClient()
    client.validate_config()
    state = await auth_flow.create_state(
        session,
        provider="wecom",
        entry=entry,
        return_to=return_to,
    )
    return {"url": client.build_login_url(entry=entry, state=state)}


@router.get("/api/auth/wecom/callback")
async def wecom_callback(
    code: str | None = Query(default=None),
    state: str | None = Query(default=None),
    session: AsyncSession = Depends(get_session),
) -> RedirectResponse:
    if not code or not state:
        raise HTTPException(status_code=400, detail="Missing WeCom callback parameters")

    client = WeComClient()
    client.validate_config()
    flow_state = await auth_flow.consume_state(session, state)
    profile = await client.get_userinfo(code)
    userid = profile.get("userid") or profile.get("UserId")
    if not userid:
        raise HTTPException(status_code=403, detail="Enterprise member is required")

    member_profile = await client.get_user(userid)
    merged_profile = {**profile, **member_profile}
    department_path = client.resolve_department_path(merged_profile)
    if inspect.isawaitable(department_path):
        department_path = await department_path
    merged_profile["department_path"] = department_path

    user = await portal_users.login_wecom_user(
        session,
        corp_id=settings.wecom_corp_id,
        userid=userid,
        profile=merged_profile,
    )
    signed_session = await portal_sessions.create_session(session, user)
    response = RedirectResponse(flow_state.return_to)
    response.set_cookie(
        portal_sessions.SESSION_COOKIE_NAME,
        signed_session,
        max_age=settings.portal_session_days * 24 * 60 * 60,
        httponly=True,
        samesite="lax",
        secure=settings.portal_cookie_secure,
    )
    return response


@router.get("/api/me")
async def me(
    tokusage_session: str | None = Cookie(default=None),
    session: AsyncSession = Depends(get_session),
) -> dict:
    user = await portal_sessions.load_user_from_signed_session(session, tokusage_session)
    if user is None:
        raise HTTPException(status_code=401, detail="not authenticated")

    token = (
        await session.execute(
            select(UserToken)
            .where(UserToken.user_id == user.id)
            .where(UserToken.active.is_(True))
            .where(UserToken.revoked_at.is_(None))
            .limit(1)
        )
    ).scalar_one_or_none()
    if token is None:
        token = await portal_users.ensure_active_token(session, user)
        await session.commit()

    return {
        "id": user.id,
        "name": user.name,
        "avatar_url": user.avatar_url,
        "secondary_department": user.secondary_department,
        "department_path": user.department_path_json or [],
        "plain_token": token.plain_token,
        "token_hint": token.token_hint,
    }


@router.post("/api/logout")
async def logout(
    response: Response,
    tokusage_session: str | None = Cookie(default=None),
    session: AsyncSession = Depends(get_session),
) -> dict[str, bool]:
    await portal_sessions.revoke_session(session, tokusage_session)
    response.delete_cookie(portal_sessions.SESSION_COOKIE_NAME)
    return {"ok": True}
