import datetime as dt
import os

os.environ["TOKUSAGE_DATABASE_URL"] = "sqlite+aiosqlite:///:memory:"

import pytest  # noqa: E402
from httpx import ASGITransport, AsyncClient  # noqa: E402
from sqlalchemy import select  # noqa: E402
from sqlalchemy.ext.asyncio import async_sessionmaker, create_async_engine  # noqa: E402

from app import db, main, models, security  # noqa: E402


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
