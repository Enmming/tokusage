"""Server-rendered portal pages."""

from __future__ import annotations

from pathlib import Path

from fastapi import APIRouter, Cookie, Depends, Request
from fastapi.responses import RedirectResponse
from fastapi.templating import Jinja2Templates
from sqlalchemy.ext.asyncio import AsyncSession

from ..db import get_session
from ..services import portal_sessions


BASE_DIR = Path(__file__).resolve().parents[1]
templates = Jinja2Templates(directory=str(BASE_DIR / "templates"))
templates.env.globals["static_version"] = "20260611-period-days"
router = APIRouter()


@router.get("/login")
async def login_page(request: Request):
    return templates.TemplateResponse(request=request, name="login.html")


@router.get("/dashboard")
async def dashboard_page(
    request: Request,
    tokusage_session: str | None = Cookie(default=None),
    session: AsyncSession = Depends(get_session),
):
    user = await portal_sessions.load_user_from_signed_session(session, tokusage_session)
    if user is None:
        return RedirectResponse("/login")
    return templates.TemplateResponse(
        request=request,
        name="dashboard.html",
        context={"user_name": user.name},
    )
