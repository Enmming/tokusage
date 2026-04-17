"""Runtime settings loaded from environment variables / .env."""

from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", env_prefix="TOKUSAGE_", extra="ignore")

    database_url: str = "postgresql+asyncpg://tokusage:tokusage@localhost:5432/tokusage"
    # Comma-separated bearer tokens. In production you'd back this with a
    # tokens table; for Phase 1 a simple env var keeps onboarding trivial.
    valid_tokens: str = "devtoken"

    @property
    def valid_tokens_set(self) -> set[str]:
        return {t.strip() for t in self.valid_tokens.split(",") if t.strip()}


settings = Settings()
