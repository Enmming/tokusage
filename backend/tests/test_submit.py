"""End-to-end tests for DB-backed token auth and raw event ingest."""

import hashlib
import os

os.environ["TOKUSAGE_DATABASE_URL"] = "sqlite+aiosqlite:///:memory:"

import pytest  # noqa: E402
from httpx import ASGITransport, AsyncClient  # noqa: E402
from sqlalchemy import select  # noqa: E402
from sqlalchemy.ext.asyncio import async_sessionmaker, create_async_engine  # noqa: E402

from app import db, main, models  # noqa: E402


def _token_hash(token: str) -> str:
    return hashlib.sha256(token.encode("utf-8")).hexdigest()


def _model(name: str):
    model = getattr(models, name, None)
    assert model is not None, f"{name} model missing"
    return model


async def _create_user_token(token: str = "dbtoken") -> int:
    UserToken = _model("UserToken")
    async with db.SessionLocal() as session:
        user_token = UserToken(
            team_id="team-1",
            user_label="alice",
            token_hash=_token_hash(token),
            token_hint=f"{token[:3]}...{token[-2:]}",
            active=True,
        )
        session.add(user_token)
        await session.commit()
        await session.refresh(user_token)
        return user_token.id


def _headers(token: str = "dbtoken") -> dict[str, str]:
    return {"Authorization": f"Bearer {token}"}


def payload(
    *,
    event_key: str = "claude:req_abc:msg_xyz",
    input_tokens: int = 6,
    output_tokens: int = 197,
    session_key: str = "claude:sha256:abcd1234",
    seq: int = 128,
    raw_payload: dict | None = None,
) -> dict:
    return {
        "client_version": "0.2.0",
        "submitted_at": "2026-04-23T10:30:00Z",
        "events": [
            {
                "source": "claude",
                "event_key": event_key,
                "event_ts": "2026-04-23T10:28:41Z",
                "session_key": session_key,
                "seq": seq,
                "model": "claude-opus-4-7",
                "provider": "anthropic",
                "tokens": {
                    "input": input_tokens,
                    "output": output_tokens,
                    "cache_read": 16757,
                    "cache_write": 10792,
                    "reasoning": 0,
                },
                "cost_cents": 0.1,
                "raw_payload": raw_payload
                or {"request_id": "req_abc", "message_id": "msg_xyz"},
            }
        ],
    }


@pytest.fixture
async def client():
    db.engine = create_async_engine(os.environ["TOKUSAGE_DATABASE_URL"])
    db.SessionLocal = async_sessionmaker(db.engine, expire_on_commit=False)
    async with db.engine.begin() as conn:
        await conn.run_sync(db.Base.metadata.create_all)
    transport = ASGITransport(app=main.app)
    async with AsyncClient(transport=transport, base_url="http://test") as c:
        yield c
    await db.engine.dispose()


async def test_rejects_unknown_db_token(client):
    response = await client.post(
        "/api/submit",
        headers=_headers("wrong"),
        json=payload(),
    )
    assert response.status_code == 401


async def test_rejects_submit_when_events_field_is_missing(client):
    await _create_user_token()

    response = await client.post(
        "/api/submit",
        headers=_headers(),
        json={
            "client_version": "0.2.0",
            "submitted_at": "2026-04-23T10:30:00Z",
        },
    )

    assert response.status_code == 422


async def test_accepts_valid_db_token_and_inserts_raw_event(client):
    user_token_id = await _create_user_token()

    response = await client.post(
        "/api/submit",
        headers=_headers(),
        json=payload(),
    )

    assert response.status_code == 200, response.text
    assert response.json() == {
        "ok": True,
        "received": 1,
        "inserted": 1,
        "duplicates_ignored": 0,
        "conflicts_ignored": 0,
    }

    RawUsageEvent = _model("RawUsageEvent")
    async with db.SessionLocal() as session:
        rows = (await session.execute(select(RawUsageEvent))).scalars().all()

    assert len(rows) == 1
    row = rows[0]
    assert row.user_token_id == user_token_id
    assert row.source == "claude"
    assert row.event_key == "claude:req_abc:msg_xyz"
    assert row.session_key == "claude:sha256:abcd1234"
    assert row.seq == 128
    assert row.input_tokens == 6
    assert row.output_tokens == 197
    assert row.client_version == "0.2.0"
    assert row.raw_payload_json == {"request_id": "req_abc", "message_id": "msg_xyz"}
    assert row.content_hash


async def test_ignores_duplicate_event_with_same_content(client):
    await _create_user_token()
    first_payload = payload()

    first = await client.post(
        "/api/submit",
        headers=_headers(),
        json=first_payload,
    )
    second = await client.post(
        "/api/submit",
        headers=_headers(),
        json=first_payload,
    )

    assert first.status_code == 200, first.text
    assert second.status_code == 200, second.text
    assert second.json() == {
        "ok": True,
        "received": 1,
        "inserted": 0,
        "duplicates_ignored": 1,
        "conflicts_ignored": 0,
    }

    RawUsageEvent = _model("RawUsageEvent")
    ConflictEvent = _model("ConflictEvent")
    async with db.SessionLocal() as session:
        raw_rows = (await session.execute(select(RawUsageEvent))).scalars().all()
        conflict_rows = (await session.execute(select(ConflictEvent))).scalars().all()

    assert len(raw_rows) == 1
    assert len(conflict_rows) == 0


async def test_audits_conflict_when_same_key_has_different_content(client):
    await _create_user_token()

    first = await client.post(
        "/api/submit",
        headers=_headers(),
        json=payload(),
    )
    conflicting = await client.post(
        "/api/submit",
        headers=_headers(),
        json=payload(
            input_tokens=999,
            raw_payload={"request_id": "req_abc", "message_id": "msg_other"},
        ),
    )

    assert first.status_code == 200, first.text
    assert conflicting.status_code == 200, conflicting.text
    assert conflicting.json() == {
        "ok": True,
        "received": 1,
        "inserted": 0,
        "duplicates_ignored": 0,
        "conflicts_ignored": 1,
    }

    RawUsageEvent = _model("RawUsageEvent")
    ConflictEvent = _model("ConflictEvent")
    async with db.SessionLocal() as session:
        raw_rows = (await session.execute(select(RawUsageEvent))).scalars().all()
        conflict_rows = (await session.execute(select(ConflictEvent))).scalars().all()

    assert len(raw_rows) == 1
    assert len(conflict_rows) == 1
    conflict = conflict_rows[0]
    assert conflict.source == "claude"
    assert conflict.event_key == "claude:req_abc:msg_xyz"
    assert conflict.reason == "content_mismatch"
    assert conflict.existing_content_hash != conflict.incoming_content_hash
    assert conflict.incoming_payload_json["tokens"]["input"] == 999


async def test_ignores_duplicate_when_only_session_metadata_differs(client):
    await _create_user_token()

    first = await client.post(
        "/api/submit",
        headers=_headers(),
        json=payload(),
    )
    duplicate = await client.post(
        "/api/submit",
        headers=_headers(),
        json=payload(session_key="claude:sha256:other", seq=999),
    )

    assert first.status_code == 200, first.text
    assert duplicate.status_code == 200, duplicate.text
    assert duplicate.json() == {
        "ok": True,
        "received": 1,
        "inserted": 0,
        "duplicates_ignored": 1,
        "conflicts_ignored": 0,
    }

    RawUsageEvent = _model("RawUsageEvent")
    ConflictEvent = _model("ConflictEvent")
    async with db.SessionLocal() as session:
        raw_rows = (await session.execute(select(RawUsageEvent))).scalars().all()
        conflict_rows = (await session.execute(select(ConflictEvent))).scalars().all()

    assert len(raw_rows) == 1
    assert len(conflict_rows) == 0


async def test_summary_endpoint_is_removed(client):
    response = await client.get(
        "/api/summary",
        headers={"Authorization": "Bearer envtoken"},
    )
    assert response.status_code == 404
