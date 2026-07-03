"""Dashboard aggregation helpers for portal users."""

from __future__ import annotations

import calendar
import datetime as dt
from collections import defaultdict
from decimal import Decimal
from typing import Any

from sqlalchemy import func, select
from sqlalchemy.ext.asyncio import AsyncSession

from ..models import PortalUser, RawUsageEvent, UserToken


TOKEN_COLUMNS = (
    RawUsageEvent.input_tokens,
    RawUsageEvent.output_tokens,
    RawUsageEvent.cache_read_tokens,
    RawUsageEvent.cache_write_tokens,
    RawUsageEvent.reasoning_tokens,
)


def _as_int(value: Any) -> int:
    if value is None:
        return 0
    if isinstance(value, Decimal):
        return int(value)
    return int(value)


def _as_date(value: Any) -> dt.date:
    if isinstance(value, dt.datetime):
        return value.date()
    if isinstance(value, dt.date):
        return value
    return dt.date.fromisoformat(str(value)[:10])


def _date_range(start: dt.date, end: dt.date) -> list[dt.date]:
    days = (end - start).days
    return [start + dt.timedelta(days=offset) for offset in range(days + 1)]


def _empty_daily_row(day: dt.date) -> dict[str, Any]:
    return {
        "date": day.isoformat(),
        "event_count": 0,
        "total_tokens": 0,
        "input_tokens": 0,
        "output_tokens": 0,
        "cache_read_tokens": 0,
        "cache_write_tokens": 0,
        "reasoning_tokens": 0,
    }


def _token_total_from_row(row: dict[str, Any]) -> int:
    return (
        row["input_tokens"]
        + row["output_tokens"]
        + row["cache_read_tokens"]
        + row["cache_write_tokens"]
        + row["reasoning_tokens"]
    )


async def fetch_daily_totals(
    session: AsyncSession,
    user: PortalUser,
    *,
    date_from: dt.date,
    date_to: dt.date,
) -> list[dict[str, Any]]:
    usage_date = func.date(RawUsageEvent.event_ts).label("usage_date")
    stmt = (
        select(
            usage_date,
            func.count(RawUsageEvent.id).label("event_count"),
            func.coalesce(func.sum(RawUsageEvent.input_tokens), 0).label(
                "input_tokens"
            ),
            func.coalesce(func.sum(RawUsageEvent.output_tokens), 0).label(
                "output_tokens"
            ),
            func.coalesce(func.sum(RawUsageEvent.cache_read_tokens), 0).label(
                "cache_read_tokens"
            ),
            func.coalesce(func.sum(RawUsageEvent.cache_write_tokens), 0).label(
                "cache_write_tokens"
            ),
            func.coalesce(func.sum(RawUsageEvent.reasoning_tokens), 0).label(
                "reasoning_tokens"
            ),
        )
        .join(UserToken, UserToken.id == RawUsageEvent.user_token_id)
        .where(UserToken.user_id == user.id)
        .where(func.date(RawUsageEvent.event_ts) >= date_from.isoformat())
        .where(func.date(RawUsageEvent.event_ts) <= date_to.isoformat())
        .group_by(usage_date)
        .order_by(usage_date)
    )

    result = await session.execute(stmt)
    rows = []
    for raw in result.mappings().all():
        row = {
            "date": _as_date(raw["usage_date"]).isoformat(),
            "event_count": _as_int(raw["event_count"]),
            "input_tokens": _as_int(raw["input_tokens"]),
            "output_tokens": _as_int(raw["output_tokens"]),
            "cache_read_tokens": _as_int(raw["cache_read_tokens"]),
            "cache_write_tokens": _as_int(raw["cache_write_tokens"]),
            "reasoning_tokens": _as_int(raw["reasoning_tokens"]),
        }
        row["total_tokens"] = _token_total_from_row(row)
        rows.append(row)
    return rows


async def fetch_calendar(
    session: AsyncSession,
    user: PortalUser,
    *,
    view: str,
    year: int,
    month: int | None = None,
) -> list[dict[str, Any]]:
    if view == "month":
        if month is None:
            raise ValueError("month is required for month view")
        last_day = calendar.monthrange(year, month)[1]
        date_from = dt.date(year, month, 1)
        date_to = dt.date(year, month, last_day)
    elif view == "year":
        date_from = dt.date(year, 1, 1)
        date_to = dt.date(year, 12, 31)
    else:
        raise ValueError("view must be month or year")

    daily_rows = {
        row["date"]: row
        for row in await fetch_daily_totals(
            session,
            user,
            date_from=date_from,
            date_to=date_to,
        )
    }
    return [
        daily_rows.get(day.isoformat(), _empty_daily_row(day))
        for day in _date_range(date_from, date_to)
    ]


async def fetch_overview(
    session: AsyncSession,
    user: PortalUser,
    *,
    year: int,
    month: int | None = None,
) -> dict[str, Any]:
    if month is None:
        date_from = dt.date(year, 1, 1)
        date_to = dt.date(year, 12, 31)
    else:
        date_from = dt.date(year, month, 1)
        date_to = dt.date(year, month, calendar.monthrange(year, month)[1])

    daily_rows = await fetch_daily_totals(
        session,
        user,
        date_from=date_from,
        date_to=date_to,
    )
    active_rows = [row for row in daily_rows if row["total_tokens"] > 0]
    total_tokens = sum(row["total_tokens"] for row in daily_rows)
    event_count = sum(row["event_count"] for row in daily_rows)
    active_days = len(active_rows)
    period_days = (date_to - date_from).days + 1

    return {
        "total_tokens": total_tokens,
        "event_count": event_count,
        "most_used_model": await _most_used_model(session, user, date_from, date_to),
        "peak_day": _peak_day(active_rows),
        "peak_week": _peak_week(active_rows),
        "highest_active_weekday": _highest_active_weekday(active_rows),
        "active_days": active_days,
        "period_days": period_days,
        "days_in_year": 366 if calendar.isleap(year) else 365,
        "current_streak_days": _current_streak(active_rows),
        "longest_streak_days": _longest_streak(active_rows),
        "active_day_average_tokens": total_tokens / active_days
        if active_days
        else 0,
    }


async def fetch_day_detail(
    session: AsyncSession,
    user: PortalUser,
    *,
    day: dt.date,
) -> dict[str, Any]:
    usage_date = func.date(RawUsageEvent.event_ts).label("usage_date")
    daily = await fetch_daily_totals(session, user, date_from=day, date_to=day)
    daily_row = daily[0] if daily else _empty_daily_row(day)

    stmt = (
        select(
            RawUsageEvent.source,
            RawUsageEvent.model,
            RawUsageEvent.provider,
            func.count(RawUsageEvent.id).label("event_count"),
            func.coalesce(func.sum(RawUsageEvent.input_tokens), 0).label(
                "input_tokens"
            ),
            func.coalesce(func.sum(RawUsageEvent.output_tokens), 0).label(
                "output_tokens"
            ),
            func.coalesce(func.sum(RawUsageEvent.cache_read_tokens), 0).label(
                "cache_read_tokens"
            ),
            func.coalesce(func.sum(RawUsageEvent.cache_write_tokens), 0).label(
                "cache_write_tokens"
            ),
            func.coalesce(func.sum(RawUsageEvent.reasoning_tokens), 0).label(
                "reasoning_tokens"
            ),
        )
        .join(UserToken, UserToken.id == RawUsageEvent.user_token_id)
        .where(UserToken.user_id == user.id)
        .where(usage_date == day.isoformat())
        .group_by(RawUsageEvent.source, RawUsageEvent.model, RawUsageEvent.provider)
        .order_by(RawUsageEvent.source, RawUsageEvent.model, RawUsageEvent.provider)
    )
    result = await session.execute(stmt)
    models = []
    for raw in result.mappings().all():
        row = {
            "source": raw["source"],
            "model": raw["model"],
            "provider": raw["provider"],
            "event_count": _as_int(raw["event_count"]),
            "input_tokens": _as_int(raw["input_tokens"]),
            "output_tokens": _as_int(raw["output_tokens"]),
            "cache_read_tokens": _as_int(raw["cache_read_tokens"]),
            "cache_write_tokens": _as_int(raw["cache_write_tokens"]),
            "reasoning_tokens": _as_int(raw["reasoning_tokens"]),
        }
        row["total_tokens"] = _token_total_from_row(row)
        models.append(row)

    return {
        "date": day.isoformat(),
        "total_tokens": daily_row["total_tokens"],
        "breakdown": {
            "input_tokens": daily_row["input_tokens"],
            "output_tokens": daily_row["output_tokens"],
            "cache_read_tokens": daily_row["cache_read_tokens"],
            "cache_write_tokens": daily_row["cache_write_tokens"],
            "reasoning_tokens": daily_row["reasoning_tokens"],
        },
        "models": models,
    }


async def fetch_period_models(
    session: AsyncSession,
    user: PortalUser,
    *,
    year: int,
    month: int | None = None,
) -> list[dict[str, Any]]:
    if month is None:
        date_from = dt.date(year, 1, 1)
        date_to = dt.date(year, 12, 31)
    else:
        date_from = dt.date(year, month, 1)
        date_to = dt.date(year, month, calendar.monthrange(year, month)[1])

    total_tokens = sum(func.coalesce(func.sum(column), 0) for column in TOKEN_COLUMNS)
    stmt = (
        select(
            RawUsageEvent.source,
            RawUsageEvent.model,
            RawUsageEvent.provider,
            total_tokens.label("total_tokens"),
        )
        .join(UserToken, UserToken.id == RawUsageEvent.user_token_id)
        .where(UserToken.user_id == user.id)
        .where(func.date(RawUsageEvent.event_ts) >= date_from.isoformat())
        .where(func.date(RawUsageEvent.event_ts) <= date_to.isoformat())
        .group_by(RawUsageEvent.source, RawUsageEvent.model, RawUsageEvent.provider)
        .order_by(total_tokens.desc(), RawUsageEvent.source, RawUsageEvent.model)
    )
    return [
        {
            "source": row["source"],
            "model": row["model"],
            "provider": row["provider"],
            "total_tokens": _as_int(row["total_tokens"]),
        }
        for row in (await session.execute(stmt)).mappings().all()
    ]


async def _most_used_model(
    session: AsyncSession,
    user: PortalUser,
    date_from: dt.date,
    date_to: dt.date,
) -> dict[str, Any] | None:
    total_tokens = sum(func.coalesce(func.sum(column), 0) for column in TOKEN_COLUMNS)
    stmt = (
        select(
            RawUsageEvent.model,
            total_tokens.label("total_tokens"),
        )
        .join(UserToken, UserToken.id == RawUsageEvent.user_token_id)
        .where(UserToken.user_id == user.id)
        .where(func.date(RawUsageEvent.event_ts) >= date_from.isoformat())
        .where(func.date(RawUsageEvent.event_ts) <= date_to.isoformat())
        .group_by(RawUsageEvent.model)
        .order_by(total_tokens.desc(), RawUsageEvent.model)
        .limit(1)
    )
    row = (await session.execute(stmt)).mappings().first()
    if row is None:
        return None
    return {"model": row["model"], "total_tokens": _as_int(row["total_tokens"])}


def _peak_day(rows: list[dict[str, Any]]) -> dict[str, Any] | None:
    if not rows:
        return None
    row = max(rows, key=lambda item: (item["total_tokens"], item["date"]))
    return {"date": row["date"], "total_tokens": row["total_tokens"]}


def _peak_week(rows: list[dict[str, Any]]) -> dict[str, Any] | None:
    if not rows:
        return None
    totals: dict[dt.date, int] = defaultdict(int)
    for row in rows:
        day = dt.date.fromisoformat(row["date"])
        week_start = day - dt.timedelta(days=day.weekday())
        totals[week_start] += row["total_tokens"]
    week_start, total = max(totals.items(), key=lambda item: (item[1], item[0]))
    week_end = week_start + dt.timedelta(days=6)
    return {
        "date_from": week_start.isoformat(),
        "date_to": week_end.isoformat(),
        "total_tokens": total,
    }


def _highest_active_weekday(rows: list[dict[str, Any]]) -> dict[str, Any] | None:
    if not rows:
        return None
    totals: dict[int, int] = defaultdict(int)
    for row in rows:
        day = dt.date.fromisoformat(row["date"])
        totals[day.weekday()] += row["total_tokens"]
    weekday, total = max(totals.items(), key=lambda item: (item[1], item[0]))
    return {"weekday": calendar.day_name[weekday], "total_tokens": total}


def _current_streak(rows: list[dict[str, Any]]) -> int:
    if not rows:
        return 0
    active_dates = {dt.date.fromisoformat(row["date"]) for row in rows}
    current = max(active_dates)
    streak = 0
    while current in active_dates:
        streak += 1
        current -= dt.timedelta(days=1)
    return streak


def _longest_streak(rows: list[dict[str, Any]]) -> int:
    if not rows:
        return 0
    longest = 0
    current = 0
    previous: dt.date | None = None
    for day in sorted(dt.date.fromisoformat(row["date"]) for row in rows):
        if previous is None or day == previous + dt.timedelta(days=1):
            current += 1
        else:
            current = 1
        longest = max(longest, current)
        previous = day
    return longest
