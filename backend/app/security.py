"""Security and hashing helpers."""

from __future__ import annotations

import hashlib
import json
from collections.abc import Mapping
from typing import Any


def hash_token(token: str) -> str:
    return hashlib.sha256(token.encode("utf-8")).hexdigest()


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
