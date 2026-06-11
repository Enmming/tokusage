import os

os.environ["TOKUSAGE_DATABASE_URL"] = "sqlite+aiosqlite:///:memory:"

import pytest  # noqa: E402
from httpx import ASGITransport, AsyncClient  # noqa: E402
from sqlalchemy.ext.asyncio import async_sessionmaker, create_async_engine  # noqa: E402

from app import db, main, models  # noqa: E402
from app.services import portal_sessions  # noqa: E402


@pytest.fixture
async def client(tmp_path):
    db_url = f"sqlite+aiosqlite:///{tmp_path / 'tokusage-pages-test.sqlite3'}"
    db.engine = create_async_engine(db_url)
    db.SessionLocal = async_sessionmaker(db.engine, expire_on_commit=False)
    async with db.engine.begin() as conn:
        await conn.run_sync(db.Base.metadata.create_all)
    transport = ASGITransport(app=main.app)
    async with AsyncClient(transport=transport, base_url="http://test") as c:
        yield c
    await db.engine.dispose()


async def seed_signed_session() -> str:
    async with db.SessionLocal() as session:
        user = models.PortalUser(
            wecom_corp_id="corp",
            wecom_userid="alice",
            name="Alice",
            department_path_json=["公司", "平台部"],
            secondary_department="平台部",
            status="active",
        )
        session.add(user)
        await session.commit()
        await session.refresh(user)
        return await portal_sessions.create_session(session, user)


async def test_login_page_renders(client):
    response = await client.get("/login")
    assert response.status_code == 200
    assert "企业微信" in response.text


async def test_dashboard_redirects_without_session(client):
    response = await client.get("/dashboard", follow_redirects=False)
    assert response.status_code in {302, 307}
    assert response.headers["location"].startswith("/login")


async def test_dashboard_page_renders_with_session(client):
    cookie = await seed_signed_session()
    response = await client.get(
        "/dashboard",
        cookies={portal_sessions.SESSION_COOKIE_NAME: cookie},
    )
    assert response.status_code == 200
    assert "每日活跃" in response.text
    assert "data-dashboard-root" in response.text
