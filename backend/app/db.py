"""Async SQLAlchemy engine + session factory."""

from collections.abc import AsyncIterator

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker, create_async_engine
from sqlalchemy.orm import DeclarativeBase

from .config import settings


class Base(DeclarativeBase):
    pass


engine = create_async_engine(settings.database_url, pool_pre_ping=True)
SessionLocal = async_sessionmaker(engine, expire_on_commit=False)


async def get_session() -> AsyncIterator[AsyncSession]:
    async with SessionLocal() as session:
        yield session


async def init_schema() -> None:
    """Create tables if missing. Called on app startup.

    For production you'd switch to alembic migrations; for a Phase 1
    single-schema service this is sufficient.
    """
    async with engine.begin() as conn:
        await conn.run_sync(Base.metadata.create_all)
