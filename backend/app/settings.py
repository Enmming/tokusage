from functools import lru_cache

from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", extra="ignore")

    database_url: str = "postgresql+asyncpg://tokusage:tokusage@localhost:5432/tokusage"
    tokusage_bearer_tokens: str = ""
    host: str = "0.0.0.0"
    port: int = 8080

    def token_map(self) -> dict[str, str]:
        """
        Parse `user=token,user2=token2` into {token: user}. Reversed so the
        endpoint can look up the user from the incoming Bearer token in O(1).
        Malformed entries are skipped silently so a typo in one pair doesn't
        break the whole server.
        """
        out: dict[str, str] = {}
        for pair in self.tokusage_bearer_tokens.split(","):
            pair = pair.strip()
            if "=" not in pair:
                continue
            user, token = pair.split("=", 1)
            user, token = user.strip(), token.strip()
            if user and token:
                out[token] = user
        return out


@lru_cache
def get_settings() -> Settings:
    return Settings()
