"""Pydantic schemas for /api/submit. Mirrors the CLI's SubmitPayload."""

import datetime as dt
from typing import Literal

from pydantic import BaseModel, Field


class DateRange(BaseModel):
    start: dt.date
    end: dt.date


class Meta(BaseModel):
    generated_at: dt.datetime
    client_version: str
    host_id: str = Field(min_length=1, max_length=64)
    date_range: DateRange


class TokenBreakdown(BaseModel):
    input: int = 0
    output: int = 0
    cache_read: int = 0
    cache_write: int = 0
    reasoning: int = 0


class Contribution(BaseModel):
    date: dt.date
    client: Literal["claude", "codex", "cursor"]
    model: str
    provider: str
    tokens: TokenBreakdown
    cost_cents: float = 0.0
    message_count: int = 0
    dedup_keys: list[str] = []


class SubmitPayload(BaseModel):
    meta: Meta
    contributions: list[Contribution]


class SubmitResponse(BaseModel):
    ok: bool = True
    submission_id: int
    contributions_upserted: int
