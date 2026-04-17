"""End-to-end tests hitting the app with an in-memory SQLite via httpx.

We override the async engine to point at aiosqlite for test isolation. The
UPSERT expressions in routes.py use Postgres-specific `on_conflict_do_update`;
for tests we execute them against SQLite which also supports ON CONFLICT.
Falling back to sqlite keeps CI fast and avoids needing docker in unit tests.
"""

import os

os.environ["TOKUSAGE_DATABASE_URL"] = "sqlite+aiosqlite:///:memory:"
os.environ["TOKUSAGE_VALID_TOKENS"] = "testtoken"

import pytest  # noqa: E402
from httpx import ASGITransport, AsyncClient  # noqa: E402
from sqlalchemy.ext.asyncio import async_sessionmaker, create_async_engine  # noqa: E402

from app import db, main  # noqa: E402


@pytest.fixture
async def client():
    # Fresh in-memory DB per test.
    db.engine = create_async_engine(os.environ["TOKUSAGE_DATABASE_URL"])
    db.SessionLocal = async_sessionmaker(db.engine, expire_on_commit=False)
    async with db.engine.begin() as conn:
        await conn.run_sync(db.Base.metadata.create_all)
    transport = ASGITransport(app=main.app)
    async with AsyncClient(transport=transport, base_url="http://test") as c:
        yield c
    await db.engine.dispose()


def payload(host="hostA", date="2026-04-17", tokens_in=10, dedup=("k1",)):
    return {
        "meta": {
            "generated_at": "2026-04-17T10:30:00Z",
            "client_version": "0.1.0",
            "host_id": host,
            "date_range": {"start": date, "end": date},
        },
        "contributions": [
            {
                "date": date,
                "client": "claude",
                "model": "claude-opus-4-7",
                "provider": "anthropic",
                "tokens": {
                    "input": tokens_in,
                    "output": 5,
                    "cache_read": 0,
                    "cache_write": 0,
                    "reasoning": 0,
                },
                "cost_cents": 0.1,
                "message_count": len(dedup),
                "dedup_keys": list(dedup),
            }
        ],
    }


async def test_rejects_without_bearer(client):
    r = await client.post("/api/submit", json=payload())
    assert r.status_code == 401


async def test_rejects_invalid_bearer(client):
    r = await client.post(
        "/api/submit", headers={"Authorization": "Bearer wrong"}, json=payload()
    )
    assert r.status_code == 401


async def test_accepts_and_upserts(client):
    r = await client.post(
        "/api/submit",
        headers={"Authorization": "Bearer testtoken"},
        json=payload(tokens_in=10),
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["ok"] is True
    assert body["contributions_upserted"] == 1

    # Summary should reflect the row.
    r = await client.get("/api/summary", headers={"Authorization": "Bearer testtoken"})
    assert r.status_code == 200
    s = r.json()
    assert s["rows"] == 1
    assert s["total_input_tokens"] == 10
    assert s["unique_hosts"] == 1


async def test_greatest_semantics_prevents_shrink(client):
    # Submit 10 tokens, then 5. The persisted value must be 10 (greatest).
    await client.post(
        "/api/submit",
        headers={"Authorization": "Bearer testtoken"},
        json=payload(tokens_in=10),
    )
    await client.post(
        "/api/submit",
        headers={"Authorization": "Bearer testtoken"},
        json=payload(tokens_in=5),
    )
    r = await client.get("/api/summary", headers={"Authorization": "Bearer testtoken"})
    assert r.json()["total_input_tokens"] == 10


async def test_multiple_hosts_are_counted_separately(client):
    await client.post(
        "/api/submit",
        headers={"Authorization": "Bearer testtoken"},
        json=payload(host="a"),
    )
    await client.post(
        "/api/submit",
        headers={"Authorization": "Bearer testtoken"},
        json=payload(host="b"),
    )
    r = await client.get("/api/summary", headers={"Authorization": "Bearer testtoken"})
    assert r.json()["unique_hosts"] == 2
    assert r.json()["rows"] == 2
