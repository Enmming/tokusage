import datetime as dt
import os

os.environ["TOKUSAGE_DATABASE_URL"] = "sqlite+aiosqlite:///:memory:"

import pytest  # noqa: E402
from fastapi import HTTPException  # noqa: E402
from httpx import ASGITransport, AsyncClient  # noqa: E402
from sqlalchemy import select  # noqa: E402
from sqlalchemy.ext.asyncio import async_sessionmaker, create_async_engine  # noqa: E402

from app import config, db, main, models, security  # noqa: E402
from app.services import auth_flow, portal_sessions, portal_users, wecom  # noqa: E402


@pytest.fixture
async def client(tmp_path):
    db_url = f"sqlite+aiosqlite:///{tmp_path / 'tokusage-portal-test.sqlite3'}"
    db.engine = create_async_engine(db_url)
    db.SessionLocal = async_sessionmaker(db.engine, expire_on_commit=False)
    async with db.engine.begin() as conn:
        await conn.run_sync(db.Base.metadata.create_all)
    transport = ASGITransport(app=main.app)
    async with AsyncClient(transport=transport, base_url="http://test") as c:
        yield c
    await db.engine.dispose()


async def test_portal_models_create_user_session_and_plain_token(client):
    async with db.SessionLocal() as session:
        user = models.PortalUser(
            wecom_corp_id="corp",
            wecom_userid="alice",
            name="Alice",
            avatar_url="https://example.com/a.png",
            department_path_json=["公司", "平台部", "AI 工程"],
            secondary_department="平台部",
            status="active",
        )
        session.add(user)
        await session.flush()

        token = models.UserToken(
            user_id=user.id,
            team_id="平台部",
            user_label="Alice",
            plain_token="tk_plain",
            token_hash=security.hash_token("tk_plain"),
            token_hint="tk_p...lain",
            active=True,
        )
        state = models.AuthFlowState(
            state_hash=security.hash_secret("state"),
            provider="wecom",
            entry="qr",
            return_to="/dashboard",
            expires_at=dt.datetime.now(dt.timezone.utc) + dt.timedelta(minutes=10),
        )
        session_obj = models.PortalSession(
            session_hash=security.hash_secret("session"),
            user_id=user.id,
            expires_at=dt.datetime.now(dt.timezone.utc) + dt.timedelta(days=7),
        )
        session.add_all([token, state, session_obj])
        await session.commit()

    async with db.SessionLocal() as session:
        stored = await session.scalar(
            select(models.UserToken).where(models.UserToken.plain_token == "tk_plain")
        )
        assert stored is not None
        assert stored.user_id == user.id


def test_generate_token_and_hint_are_stable_shape():
    token = security.generate_api_token()
    assert token.startswith("tk_")
    assert security.token_hint("tk_abcdefghijklmnopqrstuvwxyz").startswith("tk_")


async def test_create_and_consume_state_once(client):
    async with db.SessionLocal() as session:
        state = await auth_flow.create_state(
            session,
            provider="wecom",
            entry="qr",
            return_to="/dashboard",
        )
        row = await auth_flow.consume_state(session, state)
        assert row.provider == "wecom"
        assert row.return_to == "/dashboard"
        with pytest.raises(HTTPException):
            await auth_flow.consume_state(session, state)


def test_normalize_return_to_rejects_unsafe_paths():
    assert auth_flow.normalize_return_to(None) == "/dashboard"
    assert auth_flow.normalize_return_to("/dashboard?view=year") == "/dashboard?view=year"
    with pytest.raises(HTTPException):
        auth_flow.normalize_return_to("https://evil.example")
    with pytest.raises(HTTPException):
        auth_flow.normalize_return_to("/api/auth/wecom/callback")


def test_wecom_build_login_url(monkeypatch):
    monkeypatch.setattr(config.settings, "wecom_corp_id", "corp")
    monkeypatch.setattr(config.settings, "wecom_agent_id", "100001")
    monkeypatch.setattr(config.settings, "wecom_corp_secret", "secret")
    monkeypatch.setattr(
        config.settings,
        "wecom_redirect_uri",
        "https://tokusage.example/api/auth/wecom/callback",
    )
    url = wecom.WeComClient().build_login_url(entry="qr", state="state1")
    assert "open.work.weixin.qq.com/wwopen/sso/qrConnect" in url
    assert "appid=corp" in url
    assert "agentid=100001" in url


async def test_login_wecom_user_creates_user_department_and_token(client):
    profile = {
        "name": "Alice",
        "avatar_url": "https://example.com/a.png",
        "department_path": ["公司", "平台部", "AI 工程"],
    }
    async with db.SessionLocal() as session:
        user = await portal_users.login_wecom_user(
            session,
            corp_id="corp",
            userid="alice",
            profile=profile,
        )
        token = await session.scalar(
            select(models.UserToken).where(models.UserToken.user_id == user.id)
        )

    assert user.name == "Alice"
    assert user.department_path_json == ["公司", "平台部", "AI 工程"]
    assert user.secondary_department == "平台部"
    assert token is not None
    assert token.plain_token.startswith("tk_")
    assert token.team_id == "平台部"
    assert token.user_label == "Alice"


async def test_login_wecom_user_reuses_existing_active_token(client):
    async with db.SessionLocal() as session:
        first = await portal_users.login_wecom_user(
            session,
            corp_id="corp",
            userid="alice",
            profile={"name": "Alice"},
        )
        second = await portal_users.login_wecom_user(
            session,
            corp_id="corp",
            userid="alice",
            profile={"name": "Alice B"},
        )
        tokens = (
            await session.execute(
                select(models.UserToken).where(models.UserToken.user_id == first.id)
            )
        ).scalars().all()

    assert first.id == second.id
    assert len(tokens) == 1


async def test_create_and_require_portal_session(client):
    async with db.SessionLocal() as session:
        user = models.PortalUser(
            wecom_corp_id="corp",
            wecom_userid="alice",
            name="Alice",
            status="active",
        )
        session.add(user)
        await session.commit()
        await session.refresh(user)
        signed = await portal_sessions.create_session(session, user)
        loaded = await portal_sessions.load_user_from_signed_session(session, signed)

    assert loaded.id == user.id
