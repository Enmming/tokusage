#!/usr/bin/env python3

from __future__ import annotations

import argparse
import asyncio
import hashlib
import os
import secrets
import sys
from pathlib import Path

from sqlalchemy import text
from sqlalchemy.ext.asyncio import create_async_engine


BACKEND_ROOT = Path(__file__).resolve().parents[1]
if str(BACKEND_ROOT) not in sys.path:
    sys.path.insert(0, str(BACKEND_ROOT))

from app import models as _models  # noqa: F401
from app.config import settings
from app.db import Base


def generate_token() -> str:
    return f"tk_{secrets.token_urlsafe(32)}"


def token_hash(token: str) -> str:
    return hashlib.sha256(token.encode("utf-8")).hexdigest()


def token_hint(token: str) -> str:
    return f"{token[:4]}...{token[-4:]}"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Create one user token row and print the plaintext token once."
    )
    parser.add_argument("--team", required=True, help="team_id for the new token.")
    parser.add_argument("--user", required=True, help="user_label for the new token.")
    return parser.parse_args()


def resolve_database_url() -> str:
    return os.environ.get("TOKUSAGE_DATABASE_URL", settings.database_url)


async def _run() -> int:
    args = parse_args()
    plaintext = generate_token()
    hashed = token_hash(plaintext)
    hint = token_hint(plaintext)

    engine = create_async_engine(resolve_database_url())
    try:
        async with engine.begin() as conn:
            await conn.run_sync(Base.metadata.create_all)
            await conn.execute(
                text(
                    """
                    INSERT INTO user_tokens (
                        team_id,
                        user_label,
                        token_hash,
                        token_hint,
                        active
                    ) VALUES (
                        :team_id,
                        :user_label,
                        :token_hash,
                        :token_hint,
                        :active
                    )
                    """
                ),
                {
                    "team_id": args.team,
                    "user_label": args.user,
                    "token_hash": hashed,
                    "token_hint": hint,
                    "active": True,
                },
            )
    finally:
        await engine.dispose()

    print(f"Plaintext token (shown once): {plaintext}")
    print("Store this now; only the hash is persisted.")
    print(f"Inserted user_tokens row for team={args.team} user={args.user}.")
    return 0


def main() -> int:
    return asyncio.run(_run())


if __name__ == "__main__":
    raise SystemExit(main())
