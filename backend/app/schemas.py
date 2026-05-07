"""Pydantic schemas for raw event submit."""

import datetime as dt
import json
from typing import Any, Literal

from pydantic import BaseModel, Field, field_validator


MAX_EVENTS_PER_SUBMIT = 1000
MAX_RAW_PAYLOAD_BYTES = 64 * 1024


class TokenBreakdown(BaseModel):
    input: int = Field(default=0, ge=0)
    output: int = Field(default=0, ge=0)
    cache_read: int = Field(default=0, ge=0)
    cache_write: int = Field(default=0, ge=0)
    reasoning: int = Field(default=0, ge=0)


class SubmitEvent(BaseModel):
    source: Literal["claude", "codex", "cursor"]
    event_key: str = Field(min_length=1, max_length=255)
    event_ts: dt.datetime
    session_key: str | None = Field(default=None, max_length=255)
    seq: int | None = Field(default=None, ge=0)
    model: str = Field(min_length=1, max_length=128)
    provider: str = Field(min_length=1, max_length=64)
    tokens: TokenBreakdown
    cost_cents: float = Field(default=0.0, ge=0)
    raw_payload: dict[str, Any]

    @field_validator("raw_payload")
    @classmethod
    def raw_payload_must_be_bounded(cls, value: dict[str, Any]) -> dict[str, Any]:
        size = len(json.dumps(value, separators=(",", ":"), default=str).encode("utf-8"))
        if size > MAX_RAW_PAYLOAD_BYTES:
            raise ValueError(
                f"raw_payload must be at most {MAX_RAW_PAYLOAD_BYTES} bytes"
            )
        return value


class SubmitPayload(BaseModel):
    client_version: str = Field(min_length=1, max_length=32)
    submitted_at: dt.datetime
    events: list[SubmitEvent] = Field(max_length=MAX_EVENTS_PER_SUBMIT)


class SubmitResponse(BaseModel):
    ok: bool = True
    received: int
    inserted: int
    duplicates_ignored: int
    conflicts_ignored: int


class SummaryRow(BaseModel):
    team_id: str
    user_label: str
    usage_date: str
    source: str
    model: str
    provider: str
    event_count: int
    input_tokens: int
    output_tokens: int
    cache_read_tokens: int
    cache_write_tokens: int
    reasoning_tokens: int
    cost_cents: float
