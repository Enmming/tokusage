import datetime as dt
import os

os.environ["TOKUSAGE_DATABASE_URL"] = "sqlite+aiosqlite:///:memory:"

import pytest  # noqa: E402
from httpx import ASGITransport, AsyncClient  # noqa: E402
from sqlalchemy.ext.asyncio import async_sessionmaker, create_async_engine  # noqa: E402

from app import db, main, models, security  # noqa: E402
from app.services import dashboard, portal_sessions  # noqa: E402


@pytest.fixture
async def client(tmp_path):
    db_url = f"sqlite+aiosqlite:///{tmp_path / 'tokusage-dashboard-test.sqlite3'}"
    db.engine = create_async_engine(db_url)
    db.SessionLocal = async_sessionmaker(db.engine, expire_on_commit=False)
    async with db.engine.begin() as conn:
        await conn.run_sync(db.Base.metadata.create_all)
    transport = ASGITransport(app=main.app)
    async with AsyncClient(transport=transport, base_url="http://test") as c:
        yield c
    await db.engine.dispose()


def _utc(year: int, month: int, day: int, hour: int) -> dt.datetime:
    return dt.datetime(year, month, day, hour, tzinfo=dt.timezone.utc)


async def seed_dashboard_user(session):
    user = models.PortalUser(
        wecom_corp_id="corp",
        wecom_userid="alice",
        name="Alice",
        department_path_json=["公司", "平台部"],
        secondary_department="平台部",
        status="active",
    )
    session.add(user)
    await session.flush()

    token = models.UserToken(
        user_id=user.id,
        team_id="平台部",
        user_label="Alice",
        plain_token="tk_dashboard",
        token_hash=security.hash_token("tk_dashboard"),
        token_hint="tk_d...oard",
        active=True,
    )
    session.add(token)
    await session.flush()

    events = [
        models.RawUsageEvent(
            user_token_id=token.id,
            source="claude",
            event_key="claude:1",
            event_ts=_utc(2026, 6, 5, 9),
            session_key="claude:s1",
            seq=1,
            model="claude-sonnet-4.6",
            provider="anthropic",
            input_tokens=40,
            output_tokens=60,
            cache_read_tokens=0,
            cache_write_tokens=0,
            reasoning_tokens=0,
            cost_cents=0,
            content_hash="h1",
            raw_payload_json={},
            client_version="0.2.0",
            submitted_at=_utc(2026, 6, 5, 10),
        ),
        models.RawUsageEvent(
            user_token_id=token.id,
            source="codex",
            event_key="codex:1",
            event_ts=_utc(2026, 6, 9, 9),
            session_key="codex:s1:t1",
            seq=1,
            model="gpt-5",
            provider="openai",
            input_tokens=100,
            output_tokens=100,
            cache_read_tokens=100,
            cache_write_tokens=0,
            reasoning_tokens=0,
            cost_cents=0,
            content_hash="h2",
            raw_payload_json={},
            client_version="0.2.0",
            submitted_at=_utc(2026, 6, 9, 10),
        ),
        models.RawUsageEvent(
            user_token_id=token.id,
            source="claude",
            event_key="claude:2",
            event_ts=_utc(2026, 6, 10, 9),
            session_key="claude:s2",
            seq=1,
            model="claude-opus-4.7",
            provider="anthropic",
            input_tokens=200,
            output_tokens=100,
            cache_read_tokens=100,
            cache_write_tokens=0,
            reasoning_tokens=0,
            cost_cents=0,
            content_hash="h3",
            raw_payload_json={},
            client_version="0.2.0",
            submitted_at=_utc(2026, 6, 10, 10),
        ),
        models.RawUsageEvent(
            user_token_id=token.id,
            source="claude",
            event_key="claude:3",
            event_ts=_utc(2026, 6, 10, 11),
            session_key="claude:s2",
            seq=2,
            model="claude-opus-4.7",
            provider="anthropic",
            input_tokens=0,
            output_tokens=200,
            cache_read_tokens=0,
            cache_write_tokens=0,
            reasoning_tokens=0,
            cost_cents=0,
            content_hash="h4",
            raw_payload_json={},
            client_version="0.2.0",
            submitted_at=_utc(2026, 6, 10, 12),
        ),
    ]
    session.add_all(events)
    await session.commit()
    await session.refresh(user)
    return user


async def seed_dashboard_session() -> str:
    async with db.SessionLocal() as session:
        user = await seed_dashboard_user(session)
        return await portal_sessions.create_session(session, user)


async def test_dashboard_overview_calculates_token_metrics(client):
    async with db.SessionLocal() as session:
        user = await seed_dashboard_user(session)
        overview = await dashboard.fetch_overview(session, user, year=2026, month=6)

    assert overview["total_tokens"] == 1000
    assert overview["event_count"] == 4
    assert overview["most_used_model"] == {
        "model": "claude-opus-4.7",
        "total_tokens": 600,
    }
    assert overview["peak_day"] == {"date": "2026-06-10", "total_tokens": 600}
    assert overview["active_days"] == 3
    assert overview["current_streak_days"] == 2
    assert overview["longest_streak_days"] == 2
    assert overview["peak_week"]["total_tokens"] == 900
    assert overview["highest_active_weekday"]["weekday"] == "Wednesday"
    assert overview["active_day_average_tokens"] == pytest.approx(333.333333)


async def test_dashboard_calendar_returns_zero_filled_month_days(client):
    async with db.SessionLocal() as session:
        user = await seed_dashboard_user(session)
        rows = await dashboard.fetch_calendar(
            session,
            user,
            view="month",
            year=2026,
            month=6,
        )

    assert rows[0]["date"] == "2026-06-01"
    assert rows[-1]["date"] == "2026-06-30"
    assert len(rows) == 30
    assert rows[0]["total_tokens"] == 0
    assert next(row for row in rows if row["date"] == "2026-06-10")[
        "total_tokens"
    ] == 600


async def test_dashboard_day_detail_groups_by_source_model_provider(client):
    async with db.SessionLocal() as session:
        user = await seed_dashboard_user(session)
        detail = await dashboard.fetch_day_detail(
            session,
            user,
            day=dt.date(2026, 6, 10),
        )

    assert detail["date"] == "2026-06-10"
    assert detail["total_tokens"] == 600
    assert detail["breakdown"] == {
        "input_tokens": 200,
        "output_tokens": 300,
        "cache_read_tokens": 100,
        "cache_write_tokens": 0,
        "reasoning_tokens": 0,
    }
    assert detail["models"] == [
        {
            "source": "claude",
            "model": "claude-opus-4.7",
            "provider": "anthropic",
            "event_count": 2,
            "total_tokens": 600,
            "input_tokens": 200,
            "output_tokens": 300,
            "cache_read_tokens": 100,
            "cache_write_tokens": 0,
            "reasoning_tokens": 0,
        }
    ]


async def test_dashboard_routes_require_session(client):
    response = await client.get(
        "/api/dashboard/overview",
        params={"year": 2026, "month": 6},
    )
    assert response.status_code == 401


async def test_dashboard_overview_route_returns_current_user_metrics(client):
    cookie = await seed_dashboard_session()
    response = await client.get(
        "/api/dashboard/overview",
        params={"year": 2026, "month": 6},
        cookies={portal_sessions.SESSION_COOKIE_NAME: cookie},
    )
    assert response.status_code == 200
    assert response.json()["total_tokens"] == 1000


async def test_dashboard_calendar_route_returns_month_rows(client):
    cookie = await seed_dashboard_session()
    response = await client.get(
        "/api/dashboard/calendar",
        params={"view": "month", "year": 2026, "month": 6},
        cookies={portal_sessions.SESSION_COOKIE_NAME: cookie},
    )
    assert response.status_code == 200
    rows = response.json()
    assert len(rows) == 30
    assert rows[0]["date"] == "2026-06-01"


async def test_dashboard_day_detail_route_returns_grouped_rows(client):
    cookie = await seed_dashboard_session()
    response = await client.get(
        "/api/dashboard/day-detail",
        params={"date": "2026-06-10"},
        cookies={portal_sessions.SESSION_COOKIE_NAME: cookie},
    )
    assert response.status_code == 200
    assert response.json()["total_tokens"] == 600
