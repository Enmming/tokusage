"""FastAPI entry point."""

from contextlib import asynccontextmanager

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse

from .config import settings
from .db import init_schema
from .routes import router


@asynccontextmanager
async def lifespan(_: FastAPI):
    await init_schema()
    yield


app = FastAPI(title="tokusage", lifespan=lifespan)


@app.middleware("http")
async def limit_request_size(request: Request, call_next):
    content_length = request.headers.get("content-length")
    if content_length is not None:
        try:
            request_bytes = int(content_length)
        except ValueError:
            request_bytes = 0
        if request_bytes > settings.max_request_bytes:
            return JSONResponse(
                status_code=413,
                content={"detail": "request body too large"},
            )
    return await call_next(request)


app.include_router(router)
