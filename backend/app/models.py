"""Database tables.

Schema notes (see README for rationale):

- `daily_usage` is the canonical aggregate, one row per
  (host_id, date, client, model, provider). UPSERT on conflict with
  GREATEST() so re-sending a day never loses data.
- `submissions` is a ledger of raw POSTs for audit / replay.
"""

import datetime as dt

from sqlalchemy import (
    BigInteger,
    Date,
    DateTime,
    Float,
    Integer,
    String,
    UniqueConstraint,
    func,
)
from sqlalchemy.orm import Mapped, mapped_column

from .db import Base


class DailyUsage(Base):
    __tablename__ = "daily_usage"
    __table_args__ = (
        UniqueConstraint(
            "host_id", "date", "client", "model", "provider", name="uq_daily_usage"
        ),
    )

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    host_id: Mapped[str] = mapped_column(String(64), index=True)
    date: Mapped[dt.date] = mapped_column(Date, index=True)
    client: Mapped[str] = mapped_column(String(16), index=True)
    model: Mapped[str] = mapped_column(String(128))
    provider: Mapped[str] = mapped_column(String(32))

    input_tokens: Mapped[int] = mapped_column(BigInteger, default=0)
    output_tokens: Mapped[int] = mapped_column(BigInteger, default=0)
    cache_read_tokens: Mapped[int] = mapped_column(BigInteger, default=0)
    cache_write_tokens: Mapped[int] = mapped_column(BigInteger, default=0)
    reasoning_tokens: Mapped[int] = mapped_column(BigInteger, default=0)
    cost_cents: Mapped[float] = mapped_column(Float, default=0.0)
    message_count: Mapped[int] = mapped_column(Integer, default=0)

    first_seen_at: Mapped[dt.datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now()
    )
    last_updated_at: Mapped[dt.datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now(), onupdate=func.now()
    )


class Submission(Base):
    __tablename__ = "submissions"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    host_id: Mapped[str] = mapped_column(String(64), index=True)
    client_version: Mapped[str] = mapped_column(String(32))
    submitted_at: Mapped[dt.datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now(), index=True
    )
    contribution_count: Mapped[int] = mapped_column(Integer)
    total_tokens: Mapped[int] = mapped_column(BigInteger)
    total_cost_cents: Mapped[float] = mapped_column(Float)
