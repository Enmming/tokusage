"""initial schema

Revision ID: 0001
Revises:
Create Date: 2026-04-17
"""

from typing import Sequence

import sqlalchemy as sa
from alembic import op

revision: str = "0001"
down_revision: str | None = None
branch_labels: Sequence[str] | str | None = None
depends_on: Sequence[str] | str | None = None


def upgrade() -> None:
    op.create_table(
        "contributions",
        sa.Column("id", sa.Integer, primary_key=True, autoincrement=True),
        sa.Column("host_id", sa.String(64), nullable=False),
        sa.Column("date", sa.Date, nullable=False),
        sa.Column("client", sa.String(32), nullable=False),
        sa.Column("model", sa.String(128), nullable=False),
        sa.Column("provider", sa.String(32), nullable=False, server_default=""),
        sa.Column("tokens_input", sa.BigInteger, nullable=False, server_default="0"),
        sa.Column("tokens_output", sa.BigInteger, nullable=False, server_default="0"),
        sa.Column("tokens_cache_read", sa.BigInteger, nullable=False, server_default="0"),
        sa.Column("tokens_cache_write", sa.BigInteger, nullable=False, server_default="0"),
        sa.Column("tokens_reasoning", sa.BigInteger, nullable=False, server_default="0"),
        sa.Column("cost_cents", sa.Float, nullable=False, server_default="0"),
        sa.Column("message_count", sa.Integer, nullable=False, server_default="0"),
        sa.Column(
            "last_seen_at",
            sa.DateTime(timezone=True),
            server_default=sa.func.now(),
            nullable=False,
        ),
        sa.UniqueConstraint(
            "host_id", "date", "client", "model", "provider", name="contrib_uq"
        ),
    )
    op.create_index("ix_contributions_host_id", "contributions", ["host_id"])
    op.create_index("ix_contributions_date", "contributions", ["date"])

    op.create_table(
        "submissions",
        sa.Column("id", sa.Integer, primary_key=True, autoincrement=True),
        sa.Column("host_id", sa.String(64), nullable=False),
        sa.Column("user", sa.String(128), nullable=False),
        sa.Column("client_version", sa.String(32), nullable=False),
        sa.Column("contributions", sa.Integer, nullable=False),
        sa.Column("date_range_start", sa.Date, nullable=False),
        sa.Column("date_range_end", sa.Date, nullable=False),
        sa.Column(
            "submitted_at",
            sa.DateTime(timezone=True),
            server_default=sa.func.now(),
            nullable=False,
        ),
    )
    op.create_index("ix_submissions_host_id", "submissions", ["host_id"])


def downgrade() -> None:
    op.drop_table("submissions")
    op.drop_table("contributions")
