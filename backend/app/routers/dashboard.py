"""Portal dashboard API routes."""

from __future__ import annotations

import datetime as dt

from fastapi import APIRouter, Cookie, Depends, HTTPException, Query
from sqlalchemy.ext.asyncio import AsyncSession

from ..db import get_session
from ..models import PortalUser
from ..services import dashboard, portal_sessions


router = APIRouter(prefix="/api/dashboard")


async def require_portal_user(
    tokusage_session: str | None = Cookie(default=None),
    session: AsyncSession = Depends(get_session),
) -> PortalUser:
    user = await portal_sessions.load_user_from_signed_session(session, tokusage_session)
    if user is None:
        raise HTTPException(status_code=401, detail="not authenticated")
    return user


@router.get("/overview")
async def overview(
    year: int = Query(...),
    month: int | None = Query(default=None, ge=1, le=12),
    user: PortalUser = Depends(require_portal_user),
    session: AsyncSession = Depends(get_session),
) -> dict:
    return await dashboard.fetch_overview(session, user, year=year, month=month)


@router.get("/calendar")
async def calendar_rows(
    view: str = Query(...),
    year: int = Query(...),
    month: int | None = Query(default=None, ge=1, le=12),
    user: PortalUser = Depends(require_portal_user),
    session: AsyncSession = Depends(get_session),
) -> list[dict]:
    if view not in {"month", "year"}:
        raise HTTPException(status_code=422, detail="view must be month or year")
    if view == "month" and month is None:
        raise HTTPException(status_code=422, detail="month is required for month view")
    return await dashboard.fetch_calendar(
        session,
        user,
        view=view,
        year=year,
        month=month,
    )


@router.get("/day-detail")
async def day_detail(
    date: dt.date = Query(...),
    user: PortalUser = Depends(require_portal_user),
    session: AsyncSession = Depends(get_session),
) -> dict:
    return await dashboard.fetch_day_detail(session, user, day=date)


@router.get("/period-models")
async def period_models(
    year: int = Query(...),
    month: int | None = Query(default=None, ge=1, le=12),
    user: PortalUser = Depends(require_portal_user),
    session: AsyncSession = Depends(get_session),
) -> list[dict]:
    return await dashboard.fetch_period_models(
        session,
        user,
        year=year,
        month=month,
    )
