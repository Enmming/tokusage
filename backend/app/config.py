"""Runtime settings loaded from environment variables / .env."""

from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", env_prefix="TOKUSAGE_", extra="ignore")

    database_url: str = "postgresql+asyncpg://tokusage:tokusage@localhost:5432/tokusage"
    max_request_bytes: int = 8 * 1024 * 1024
    auth_mode: str = "wecom"
    wecom_corp_id: str = ""
    wecom_agent_id: str = ""
    wecom_corp_secret: str = ""
    wecom_redirect_uri: str = ""
    portal_session_secret: str = "dev-portal-session-secret-change-me"
    portal_session_days: int = 30
    portal_cookie_secure: bool = False


settings = Settings()
