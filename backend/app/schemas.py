"""Pydantic schemas for raw event submit."""

import datetime as dt
from typing import Any, Literal

from pydantic import BaseModel


class TokenBreakdown(BaseModel):
    input: int = 0
    output: int = 0
    cache_read: int = 0
    cache_write: int = 0
    reasoning: int = 0


class SubmitEvent(BaseModel):
    source: Literal["claude", "codex", "cursor"]
    event_key: str
    event_ts: dt.datetime
    session_key: str | None = None
    seq: int | None = None
    model: str
    provider: str
    tokens: TokenBreakdown
    cost_cents: float = 0.0
    raw_payload: dict[str, Any]


class SubmitPayload(BaseModel):
    client_version: str
    submitted_at: dt.datetime
    events: list[SubmitEvent]


class SubmitResponse(BaseModel):
    ok: bool = True
    received: int
    inserted: int
    duplicates_ignored: int
    conflicts_ignored: int
