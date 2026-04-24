import csv
import io
import sys
from pathlib import Path

import pytest
from sqlalchemy import text
from sqlalchemy.ext.asyncio import create_async_engine


SCRIPTS_DIR = Path(__file__).resolve().parents[1] / "scripts"
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

import usage_summary


async def _seed_db(database_url: str) -> None:
    engine = create_async_engine(database_url)
    async with engine.begin() as conn:
        await conn.execute(
            text(
                """
                CREATE TABLE user_tokens (
                    id INTEGER PRIMARY KEY,
                    team_id TEXT NOT NULL,
                    user_label TEXT NOT NULL,
                    token_hash TEXT NOT NULL,
                    token_hint TEXT NOT NULL,
                    active BOOLEAN NOT NULL
                )
                """
            )
        )
        await conn.execute(
            text(
                """
                CREATE TABLE raw_usage_events (
                    id INTEGER PRIMARY KEY,
                    user_token_id INTEGER NOT NULL,
                    source TEXT NOT NULL,
                    event_key TEXT NOT NULL,
                    event_ts TEXT NOT NULL,
                    session_key TEXT,
                    seq INTEGER,
                    model TEXT NOT NULL,
                    provider TEXT NOT NULL,
                    input_tokens INTEGER NOT NULL,
                    output_tokens INTEGER NOT NULL,
                    cache_read_tokens INTEGER NOT NULL,
                    cache_write_tokens INTEGER NOT NULL,
                    reasoning_tokens INTEGER NOT NULL,
                    cost_cents REAL NOT NULL,
                    content_hash TEXT NOT NULL,
                    raw_payload_json TEXT NOT NULL,
                    client_version TEXT NOT NULL,
                    submitted_at TEXT NOT NULL,
                    received_at TEXT NOT NULL
                )
                """
            )
        )

        await conn.execute(
            text(
                """
                INSERT INTO user_tokens (id, team_id, user_label, token_hash, token_hint, active)
                VALUES
                    (1, 'team-1', 'alice', 'hash-a', 'tk_a', 1),
                    (2, 'team-1', 'bob', 'hash-b', 'tk_b', 1),
                    (3, 'team-2', 'carol', 'hash-c', 'tk_c', 1)
                """
            )
        )
        await conn.execute(
            text(
                """
                INSERT INTO raw_usage_events (
                    id, user_token_id, source, event_key, event_ts, session_key, seq,
                    model, provider, input_tokens, output_tokens, cache_read_tokens,
                    cache_write_tokens, reasoning_tokens, cost_cents, content_hash,
                    raw_payload_json, client_version, submitted_at, received_at
                )
                VALUES
                    (1, 1, 'claude', 'claude:req1:msg1', '2026-04-23T10:00:00Z', NULL, NULL,
                     'claude-opus-4-7', 'anthropic', 10, 5, 100, 50, 0, 1.25, 'h1',
                     '{}', '0.2.0', '2026-04-23T10:01:00Z', '2026-04-23T10:01:00Z'),
                    (2, 1, 'claude', 'claude:req2:msg2', '2026-04-23T11:00:00Z', NULL, NULL,
                     'claude-opus-4-7', 'anthropic', 7, 3, 10, 5, 0, 0.75, 'h2',
                     '{}', '0.2.0', '2026-04-23T11:01:00Z', '2026-04-23T11:01:00Z'),
                    (3, 2, 'codex', 'codex:s1:t1', '2026-04-24T09:00:00Z', NULL, NULL,
                     'gpt-5.4-mini', 'openai', 20, 8, 0, 0, 2, 2.50, 'h3',
                     '{}', '0.2.0', '2026-04-24T09:01:00Z', '2026-04-24T09:01:00Z'),
                    (4, 3, 'cursor', 'cursor:e1', '2026-04-24T14:00:00Z', NULL, NULL,
                     'gpt-5.4', 'openai', 4, 6, 1, 2, 0, 0.40, 'h4',
                     '{}', '0.2.0', '2026-04-24T14:01:00Z', '2026-04-24T14:01:00Z')
                """
            )
        )
    await engine.dispose()


@pytest.fixture
async def database_url(tmp_path):
    db_path = tmp_path / "usage-summary.sqlite3"
    url = f"sqlite+aiosqlite:///{db_path}"
    await _seed_db(url)
    return url


@pytest.mark.asyncio
async def test_groups_by_team_user_date_source_model_provider(database_url):
    rows = await usage_summary.fetch_summary_rows(
        database_url,
        usage_summary.SummaryFilters(),
    )

    assert rows == [
        {
            "team_id": "team-1",
            "user_label": "alice",
            "usage_date": "2026-04-23",
            "source": "claude",
            "model": "claude-opus-4-7",
            "provider": "anthropic",
            "event_count": 2,
            "input_tokens": 17,
            "output_tokens": 8,
            "cache_read_tokens": 110,
            "cache_write_tokens": 55,
            "reasoning_tokens": 0,
            "cost_cents": 2.0,
        },
        {
            "team_id": "team-1",
            "user_label": "bob",
            "usage_date": "2026-04-24",
            "source": "codex",
            "model": "gpt-5.4-mini",
            "provider": "openai",
            "event_count": 1,
            "input_tokens": 20,
            "output_tokens": 8,
            "cache_read_tokens": 0,
            "cache_write_tokens": 0,
            "reasoning_tokens": 2,
            "cost_cents": 2.5,
        },
        {
            "team_id": "team-2",
            "user_label": "carol",
            "usage_date": "2026-04-24",
            "source": "cursor",
            "model": "gpt-5.4",
            "provider": "openai",
            "event_count": 1,
            "input_tokens": 4,
            "output_tokens": 6,
            "cache_read_tokens": 1,
            "cache_write_tokens": 2,
            "reasoning_tokens": 0,
            "cost_cents": 0.4,
        },
    ]


@pytest.mark.asyncio
async def test_filters_by_date_range_and_source(database_url):
    rows = await usage_summary.fetch_summary_rows(
        database_url,
        usage_summary.SummaryFilters(
            date_from="2026-04-24",
            date_to="2026-04-24",
            source="codex",
        ),
    )

    assert rows == [
        {
            "team_id": "team-1",
            "user_label": "bob",
            "usage_date": "2026-04-24",
            "source": "codex",
            "model": "gpt-5.4-mini",
            "provider": "openai",
            "event_count": 1,
            "input_tokens": 20,
            "output_tokens": 8,
            "cache_read_tokens": 0,
            "cache_write_tokens": 0,
            "reasoning_tokens": 2,
            "cost_cents": 2.5,
        }
    ]


def test_render_rows_supports_json_and_csv():
    rows = [
        {
            "team_id": "team-1",
            "user_label": "alice",
            "usage_date": "2026-04-23",
            "source": "claude",
            "model": "claude-opus-4-7",
            "provider": "anthropic",
            "event_count": 2,
            "input_tokens": 17,
            "output_tokens": 8,
            "cache_read_tokens": 110,
            "cache_write_tokens": 55,
            "reasoning_tokens": 0,
            "cost_cents": 2.0,
        }
    ]

    json_output = usage_summary.render_rows(rows, "json")
    assert '"team_id": "team-1"' in json_output

    csv_output = usage_summary.render_rows(rows, "csv")
    parsed = list(csv.DictReader(io.StringIO(csv_output)))
    assert parsed == [
        {
            "team_id": "team-1",
            "user_label": "alice",
            "usage_date": "2026-04-23",
            "source": "claude",
            "model": "claude-opus-4-7",
            "provider": "anthropic",
            "event_count": "2",
            "input_tokens": "17",
            "output_tokens": "8",
            "cache_read_tokens": "110",
            "cache_write_tokens": "55",
            "reasoning_tokens": "0",
            "cost_cents": "2.0",
        }
    ]
