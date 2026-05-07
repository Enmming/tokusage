"""Runtime settings loaded from environment variables / .env."""

from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", env_prefix="TOKUSAGE_", extra="ignore")

    database_url: str = "postgresql+asyncpg://tokusage:tokusage@localhost:5432/tokusage"
    max_request_bytes: int = 8 * 1024 * 1024


settings = Settings()
