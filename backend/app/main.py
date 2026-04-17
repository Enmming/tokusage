"""FastAPI entry point."""

from contextlib import asynccontextmanager

from fastapi import FastAPI

from .db import init_schema
from .routes import router


@asynccontextmanager
async def lifespan(_: FastAPI):
    await init_schema()
    yield


app = FastAPI(title="tokusage", lifespan=lifespan)
app.include_router(router)
