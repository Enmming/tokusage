"""Database tables for token auth and raw usage event ingest."""

import datetime as dt

from sqlalchemy import (
    JSON,
    BigInteger,
    Boolean,
    DateTime,
    Float,
    ForeignKey,
    Index,
    Integer,
    String,
    UniqueConstraint,
    func,
)
from sqlalchemy.orm import Mapped, mapped_column

from .db import Base


class UserToken(Base):
    __tablename__ = "user_tokens"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    team_id: Mapped[str] = mapped_column(String(64), index=True)
    user_label: Mapped[str] = mapped_column(String(128), index=True)
    token_hash: Mapped[str] = mapped_column(String(64), unique=True, index=True)
    token_hint: Mapped[str] = mapped_column(String(32))
    active: Mapped[bool] = mapped_column(Boolean, default=True)
    created_at: Mapped[dt.datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now()
    )
    revoked_at: Mapped[dt.datetime | None] = mapped_column(DateTime(timezone=True))
    last_used_at: Mapped[dt.datetime | None] = mapped_column(DateTime(timezone=True))


class RawUsageEvent(Base):
    __tablename__ = "raw_usage_events"
    __table_args__ = (
        UniqueConstraint(
            "user_token_id",
            "source",
            "event_key",
            name="uq_raw_usage_events_user_source_event_key",
        ),
        Index("ix_raw_usage_events_user_token_event_ts", "user_token_id", "event_ts"),
        Index("ix_raw_usage_events_source_event_ts", "source", "event_ts"),
        Index("ix_raw_usage_events_session_key_seq", "session_key", "seq"),
    )

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    user_token_id: Mapped[int] = mapped_column(
        ForeignKey("user_tokens.id"), index=True
    )
    source: Mapped[str] = mapped_column(String(16), index=True)
    event_key: Mapped[str] = mapped_column(String(255))
    event_ts: Mapped[dt.datetime] = mapped_column(DateTime(timezone=True), index=True)
    session_key: Mapped[str | None] = mapped_column(String(255))
    seq: Mapped[int | None] = mapped_column(Integer)
    model: Mapped[str] = mapped_column(String(128))
    provider: Mapped[str] = mapped_column(String(64))
    input_tokens: Mapped[int] = mapped_column(BigInteger, default=0)
    output_tokens: Mapped[int] = mapped_column(BigInteger, default=0)
    cache_read_tokens: Mapped[int] = mapped_column(BigInteger, default=0)
    cache_write_tokens: Mapped[int] = mapped_column(BigInteger, default=0)
    reasoning_tokens: Mapped[int] = mapped_column(BigInteger, default=0)
    cost_cents: Mapped[float] = mapped_column(Float, default=0.0)
    content_hash: Mapped[str] = mapped_column(String(64))
    raw_payload_json: Mapped[dict] = mapped_column(JSON)
    client_version: Mapped[str] = mapped_column(String(32))
    submitted_at: Mapped[dt.datetime] = mapped_column(DateTime(timezone=True))
    received_at: Mapped[dt.datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now()
    )


class ConflictEvent(Base):
    __tablename__ = "conflict_events"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    user_token_id: Mapped[int] = mapped_column(
        ForeignKey("user_tokens.id"), index=True
    )
    source: Mapped[str] = mapped_column(String(16), index=True)
    event_key: Mapped[str] = mapped_column(String(255), index=True)
    existing_event_id: Mapped[int] = mapped_column(ForeignKey("raw_usage_events.id"))
    existing_content_hash: Mapped[str] = mapped_column(String(64))
    incoming_content_hash: Mapped[str] = mapped_column(String(64))
    incoming_payload_json: Mapped[dict] = mapped_column(JSON)
    client_version: Mapped[str] = mapped_column(String(32))
    submitted_at: Mapped[dt.datetime] = mapped_column(DateTime(timezone=True))
    detected_at: Mapped[dt.datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now()
    )
    reason: Mapped[str] = mapped_column(String(64), default="content_mismatch")
