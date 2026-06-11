"""Security and hashing helpers."""

from __future__ import annotations

import hashlib
import hmac
import json
import secrets
from collections.abc import Mapping
from typing import Any


def hash_token(token: str) -> str:
    return hashlib.sha256(token.encode("utf-8")).hexdigest()


def generate_api_token() -> str:
    return f"tk_{secrets.token_urlsafe(32)}"


def token_hint(token: str) -> str:
    return f"{token[:4]}...{token[-4:]}"


def hash_secret(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def sign_value(value: str, secret: str) -> str:
    sig = hmac.new(
        secret.encode("utf-8"),
        value.encode("utf-8"),
        hashlib.sha256,
    ).hexdigest()
    return f"{value}.{sig}"


def unsign_value(signed: str, secret: str) -> str | None:
    try:
        value, sig = signed.rsplit(".", 1)
    except ValueError:
        return None
    expected = hmac.new(
        secret.encode("utf-8"),
        value.encode("utf-8"),
        hashlib.sha256,
    ).hexdigest()
    if not hmac.compare_digest(sig, expected):
        return None
    return value


def _normalize_json_value(value: Any) -> Any:
    if isinstance(value, Mapping):
        return {key: _normalize_json_value(value[key]) for key in sorted(value)}
    if isinstance(value, list):
        return [_normalize_json_value(item) for item in value]
    return value


def normalize_content(content: Mapping[str, Any]) -> str:
    normalized = _normalize_json_value(content)
    return json.dumps(normalized, sort_keys=True, separators=(",", ":"))


def hash_content(content: Mapping[str, Any]) -> str:
    return hashlib.sha256(normalize_content(content).encode("utf-8")).hexdigest()
