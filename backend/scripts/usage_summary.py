#!/usr/bin/env python3

from __future__ import annotations

import argparse
import asyncio
import csv
import io
import json
import os
import sys
from dataclasses import dataclass
from datetime import date
from decimal import Decimal
from pathlib import Path
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import create_async_engine


BACKEND_ROOT = Path(__file__).resolve().parents[1]
if str(BACKEND_ROOT) not in sys.path:
    sys.path.insert(0, str(BACKEND_ROOT))

from app.config import settings


SUMMARY_COLUMNS = [
    "team_id",
    "user_label",
    "usage_date",
    "source",
    "model",
    "provider",
    "event_count",
    "input_tokens",
    "output_tokens",
    "cache_read_tokens",
    "cache_write_tokens",
    "reasoning_tokens",
    "cost_cents",
]

FILTER_COLUMN_MAP = {
    "team": "ut.team_id",
    "user": "ut.user_label",
    "source": "rue.source",
    "model": "rue.model",
    "provider": "rue.provider",
}


@dataclass(frozen=True)
class SummaryFilters:
    date_from: str | None = None
    date_to: str | None = None
    team: str | None = None
    user: str | None = None
    source: str | None = None
    model: str | None = None
    provider: str | None = None


def build_summary_query(filters: SummaryFilters):
    where_clauses: list[str] = []
    params: dict[str, Any] = {}

    if filters.date_from:
        where_clauses.append("DATE(rue.event_ts) >= :date_from")
        params["date_from"] = filters.date_from
    if filters.date_to:
        where_clauses.append("DATE(rue.event_ts) <= :date_to")
        params["date_to"] = filters.date_to

    for field_name, column_name in FILTER_COLUMN_MAP.items():
        value = getattr(filters, field_name)
        if value:
            where_clauses.append(f"{column_name} = :{field_name}")
            params[field_name] = value

    where_sql = ""
    if where_clauses:
        where_sql = "WHERE " + " AND ".join(where_clauses)

    query = text(
        f"""
        SELECT
            ut.team_id,
            ut.user_label,
            DATE(rue.event_ts) AS usage_date,
            rue.source,
            rue.model,
            rue.provider,
            COUNT(*) AS event_count,
            COALESCE(SUM(rue.input_tokens), 0) AS input_tokens,
            COALESCE(SUM(rue.output_tokens), 0) AS output_tokens,
            COALESCE(SUM(rue.cache_read_tokens), 0) AS cache_read_tokens,
            COALESCE(SUM(rue.cache_write_tokens), 0) AS cache_write_tokens,
            COALESCE(SUM(rue.reasoning_tokens), 0) AS reasoning_tokens,
            COALESCE(SUM(rue.cost_cents), 0) AS cost_cents
        FROM raw_usage_events rue
        JOIN user_tokens ut ON ut.id = rue.user_token_id
        {where_sql}
        GROUP BY
            ut.team_id,
            ut.user_label,
            DATE(rue.event_ts),
            rue.source,
            rue.model,
            rue.provider
        ORDER BY
            DATE(rue.event_ts),
            ut.team_id,
            ut.user_label,
            rue.source,
            rue.model,
            rue.provider
        """
    )
    return query, params


def _normalize_value(value: Any) -> Any:
    if isinstance(value, date):
        return value.isoformat()
    if isinstance(value, Decimal):
        return float(value)
    return value


async def fetch_summary_rows(
    database_url: str,
    filters: SummaryFilters,
) -> list[dict[str, Any]]:
    engine = create_async_engine(database_url)
    query, params = build_summary_query(filters)
    try:
        async with engine.connect() as conn:
            result = await conn.execute(query, params)
            rows = result.mappings().all()
    finally:
        await engine.dispose()

    normalized_rows: list[dict[str, Any]] = []
    for row in rows:
        normalized_row = {
            column: _normalize_value(row[column])
            for column in SUMMARY_COLUMNS
        }
        normalized_rows.append(normalized_row)
    return normalized_rows


def render_rows(rows: list[dict[str, Any]], output_format: str) -> str:
    if output_format == "json":
        return json.dumps(rows, indent=2)
    if output_format == "csv":
        buffer = io.StringIO()
        writer = csv.DictWriter(buffer, fieldnames=SUMMARY_COLUMNS)
        writer.writeheader()
        for row in rows:
            writer.writerow(row)
        return buffer.getvalue()
    return render_table(rows)


def render_table(rows: list[dict[str, Any]]) -> str:
    if not rows:
        return "No rows matched."

    widths = {
        column: max(len(column), *(len(str(row[column])) for row in rows))
        for column in SUMMARY_COLUMNS
    }
    header = " | ".join(column.ljust(widths[column]) for column in SUMMARY_COLUMNS)
    separator = "-+-".join("-" * widths[column] for column in SUMMARY_COLUMNS)
    body = [
        " | ".join(str(row[column]).ljust(widths[column]) for column in SUMMARY_COLUMNS)
        for row in rows
    ]
    return "\n".join([header, separator, *body])


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Summarize raw_usage_events by team, user, date, source, model, and provider."
    )
    parser.add_argument("--from", dest="date_from", help="Inclusive start date (YYYY-MM-DD).")
    parser.add_argument("--to", dest="date_to", help="Inclusive end date (YYYY-MM-DD).")
    parser.add_argument("--team", help="Filter by team_id.")
    parser.add_argument("--user", help="Filter by user_label.")
    parser.add_argument("--source", help="Filter by source.")
    parser.add_argument("--model", help="Filter by model.")
    parser.add_argument("--provider", help="Filter by provider.")
    parser.add_argument(
        "--format",
        choices=("table", "json", "csv"),
        default="table",
        help="Output format.",
    )
    return parser.parse_args()


def resolve_database_url() -> str:
    return os.environ.get("TOKUSAGE_DATABASE_URL", settings.database_url)


async def _run() -> int:
    args = parse_args()
    filters = SummaryFilters(
        date_from=args.date_from,
        date_to=args.date_to,
        team=args.team,
        user=args.user,
        source=args.source,
        model=args.model,
        provider=args.provider,
    )
    rows = await fetch_summary_rows(resolve_database_url(), filters)
    print(render_rows(rows, args.format))
    return 0


def main() -> int:
    return asyncio.run(_run())


if __name__ == "__main__":
    raise SystemExit(main())
