"""Enterprise WeChat API client for portal login."""

from __future__ import annotations

from time import monotonic
from typing import Any
from urllib.parse import urlencode

import httpx
from fastapi import HTTPException

from ..config import settings


class WeComClient:
    _access_token: str = ""
    _access_token_expires_at: float = 0

    def validate_config(self) -> None:
        missing = [
            name
            for name, value in {
                "TOKUSAGE_WECOM_CORP_ID": settings.wecom_corp_id,
                "TOKUSAGE_WECOM_AGENT_ID": settings.wecom_agent_id,
                "TOKUSAGE_WECOM_CORP_SECRET": settings.wecom_corp_secret,
                "TOKUSAGE_WECOM_REDIRECT_URI": settings.wecom_redirect_uri,
            }.items()
            if not value
        ]
        if missing:
            raise HTTPException(
                status_code=500,
                detail=f"Missing WeCom config: {', '.join(missing)}",
            )

    def build_login_url(self, *, entry: str, state: str) -> str:
        self.validate_config()
        if entry not in {"qr", "oauth"}:
            raise HTTPException(status_code=400, detail="Invalid WeCom entry")

        if entry == "qr":
            return "https://open.work.weixin.qq.com/wwopen/sso/qrConnect?" + urlencode(
                {
                    "appid": settings.wecom_corp_id,
                    "agentid": settings.wecom_agent_id,
                    "redirect_uri": settings.wecom_redirect_uri,
                    "state": state,
                }
            )

        return (
            "https://open.weixin.qq.com/connect/oauth2/authorize?"
            + urlencode(
                {
                    "appid": settings.wecom_corp_id,
                    "redirect_uri": settings.wecom_redirect_uri,
                    "response_type": "code",
                    "scope": "snsapi_base",
                    "state": state,
                    "agentid": settings.wecom_agent_id,
                }
            )
            + "#wechat_redirect"
        )

    async def access_token(self) -> str:
        self.validate_config()
        cls = type(self)
        if cls._access_token and monotonic() < cls._access_token_expires_at:
            return cls._access_token

        async with httpx.AsyncClient(timeout=10) as client:
            resp = await client.get(
                "https://qyapi.weixin.qq.com/cgi-bin/gettoken",
                params={
                    "corpid": settings.wecom_corp_id,
                    "corpsecret": settings.wecom_corp_secret,
                },
            )
        data = resp.json()
        if data.get("errcode") != 0:
            raise HTTPException(status_code=502, detail="WeCom gettoken failed")

        cls._access_token = data["access_token"]
        cls._access_token_expires_at = monotonic() + max(
            int(data.get("expires_in", 7200)) - 120,
            60,
        )
        return cls._access_token

    async def get_userinfo(self, code: str) -> dict[str, Any]:
        token = await self.access_token()
        async with httpx.AsyncClient(timeout=10) as client:
            resp = await client.get(
                "https://qyapi.weixin.qq.com/cgi-bin/auth/getuserinfo",
                params={"access_token": token, "code": code},
            )
        data = resp.json()
        if data.get("errcode") != 0:
            raise HTTPException(status_code=502, detail="WeCom getuserinfo failed")
        return data

    async def get_user(self, userid: str) -> dict[str, Any]:
        token = await self.access_token()
        async with httpx.AsyncClient(timeout=10) as client:
            resp = await client.get(
                "https://qyapi.weixin.qq.com/cgi-bin/user/get",
                params={"access_token": token, "userid": userid},
            )
        data = resp.json()
        if data.get("errcode") != 0:
            raise HTTPException(status_code=502, detail="WeCom get user failed")
        if avatar := data.get("avatar"):
            data["avatar_url"] = avatar
        return data

    async def get_department_list(self) -> list[dict[str, Any]]:
        token = await self.access_token()
        async with httpx.AsyncClient(timeout=10) as client:
            resp = await client.get(
                "https://qyapi.weixin.qq.com/cgi-bin/department/list",
                params={"access_token": token},
            )
        data = resp.json()
        if data.get("errcode") != 0:
            raise HTTPException(status_code=502, detail="WeCom department list failed")
        departments = data.get("department", [])
        return departments if isinstance(departments, list) else []

    async def resolve_department_path(self, member_profile: dict[str, Any]) -> list[str]:
        if path := member_profile.get("department_path"):
            if isinstance(path, list):
                return [str(part) for part in path if str(part)]
            if isinstance(path, str):
                return [part for part in path.split("/") if part]

        department_ids = member_profile.get("department") or []
        if not isinstance(department_ids, list) or not department_ids:
            return []
        primary_department = member_profile.get("main_department") or department_ids[0]

        departments = await self.get_department_list()
        by_id = {item.get("id"): item for item in departments}
        path: list[str] = []
        current_id = primary_department
        seen: set[Any] = set()
        while current_id in by_id and current_id not in seen:
            seen.add(current_id)
            item = by_id[current_id]
            name = item.get("name")
            if name:
                path.append(str(name))
            parent_id = item.get("parentid")
            if not parent_id or parent_id == current_id:
                break
            current_id = parent_id

        return list(reversed(path))
