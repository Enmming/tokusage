# WeCom Dashboard Portal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a FastAPI-served Enterprise WeChat portal where users log in, receive/view their personal API token, and inspect their own token usage dashboard.

**Architecture:** Add portal identity and session tables alongside existing bearer-token authentication. Keep CLI/API submit auth based on `user_tokens.token_hash`; add session-cookie auth for browser pages and dashboard APIs. Serve the first UI as Jinja templates plus lightweight JavaScript, with dashboard data computed from `raw_usage_events` joined through the current portal user's tokens.

**Tech Stack:** FastAPI, SQLAlchemy async ORM, Pydantic settings, httpx for WeCom API calls, Jinja2 templates, vanilla browser JavaScript, pytest + httpx ASGITransport for tests.

---

## Reference Documents

- Spec: `docs/superpowers/specs/2026-06-11-wecom-dashboard-portal-design.md`
- Existing bearer auth: `backend/app/auth.py`
- Existing routes: `backend/app/routes.py`
- Existing models: `backend/app/models.py`
- Existing summary query: `backend/app/summary.py`
- WeCom reference implementation:
  - `/Users/gd/zy-research/backend/app/routers/auth.py`
  - `/Users/gd/zy-research/backend/app/services/wecom.py`
  - `/Users/gd/zy-research/backend/app/services/auth_flow.py`

## File Structure

Create focused modules instead of growing `backend/app/routes.py`:

- `backend/app/models.py`
  - Add `PortalUser`, `AuthFlowState`, `PortalSession`.
  - Extend `UserToken` with `user_id` and `plain_token`.
- `backend/app/config.py`
  - Add WeCom and portal session settings.
- `backend/app/security.py`
  - Add token generation, token hint, secret hashing, and HMAC helpers.
- `backend/app/services/auth_flow.py`
  - State creation/consumption and safe return path validation.
- `backend/app/services/wecom.py`
  - WeCom login URL construction and API client calls.
- `backend/app/services/portal_users.py`
  - Resolve/create WeCom user, store department path, create active API token.
- `backend/app/services/portal_sessions.py`
  - Create, validate, revoke session cookies.
- `backend/app/services/dashboard.py`
  - Dashboard aggregate queries and metric calculations.
- `backend/app/routers/auth.py`
  - `/api/auth/wecom/login-url`, `/api/auth/wecom/callback`, `/api/me`, `/api/logout`.
- `backend/app/routers/dashboard.py`
  - `/api/dashboard/overview`, `/api/dashboard/calendar`, `/api/dashboard/day-detail`.
- `backend/app/routers/pages.py`
  - `/login`, `/dashboard`, auth failure page.
- `backend/app/templates/login.html`
  - Login shell that starts WeCom login.
- `backend/app/templates/dashboard.html`
  - A-style single-page dashboard.
- `backend/app/static/dashboard.js`
  - Fetch dashboard APIs, render heatmap, line chart, day detail, avatar menu.
- `backend/app/main.py`
  - Include new routers and mount static files.
- `backend/pyproject.toml`
  - Add runtime dependencies: `httpx`, `jinja2`.
- Tests:
  - `backend/tests/test_portal_auth.py`
  - `backend/tests/test_dashboard_api.py`
  - `backend/tests/test_portal_pages.py`
  - Update `backend/tests/test_submit.py` for nullable/new `UserToken` fields if needed.

## Task 1: Add Portal Data Model And Settings

**Files:**
- Modify: `backend/app/models.py`
- Modify: `backend/app/config.py`
- Modify: `backend/app/security.py`
- Test: `backend/tests/test_portal_auth.py`

- [ ] **Step 1: Write failing tests for portal models and token helpers**

Create `backend/tests/test_portal_auth.py` with the same SQLite setup pattern as `backend/tests/test_submit.py`.

Test cases:

```python
async def test_portal_models_create_user_session_and_plain_token(client):
    async with db.SessionLocal() as session:
        user = models.PortalUser(
            wecom_corp_id="corp",
            wecom_userid="alice",
            name="Alice",
            avatar_url="https://example.com/a.png",
            department_path_json=["公司", "平台部", "AI 工程"],
            secondary_department="平台部",
            status="active",
        )
        session.add(user)
        await session.flush()
        token = models.UserToken(
            user_id=user.id,
            team_id="平台部",
            user_label="Alice",
            plain_token="tk_plain",
            token_hash=security.hash_token("tk_plain"),
            token_hint="tk_p...lain",
            active=True,
        )
        state = models.AuthFlowState(
            state_hash=security.hash_secret("state"),
            provider="wecom",
            entry="qr",
            return_to="/dashboard",
            expires_at=dt.datetime.now(dt.timezone.utc) + dt.timedelta(minutes=10),
        )
        session_obj = models.PortalSession(
            session_hash=security.hash_secret("session"),
            user_id=user.id,
            expires_at=dt.datetime.now(dt.timezone.utc) + dt.timedelta(days=7),
        )
        session.add_all([token, state, session_obj])
        await session.commit()

    async with db.SessionLocal() as session:
        stored = await session.scalar(select(models.UserToken).where(models.UserToken.plain_token == "tk_plain"))
        assert stored is not None
        assert stored.user_id == user.id
```

Also add pure helper tests:

```python
def test_generate_token_and_hint_are_stable_shape():
    token = security.generate_api_token()
    assert token.startswith("tk_")
    assert security.token_hint("tk_abcdefghijklmnopqrstuvwxyz").startswith("tk_")
```

- [ ] **Step 2: Run model/helper tests to verify they fail**

Run: `cd backend && uv run pytest tests/test_portal_auth.py -q`

Expected: FAIL because `PortalUser`, `PortalSession`, `AuthFlowState`, `plain_token`, `user_id`, and helper functions do not exist.

- [ ] **Step 3: Implement models and settings**

In `backend/app/models.py`:

- Add `PortalUser` with integer primary key for consistency with existing models.
- Add unique constraint on `(wecom_corp_id, wecom_userid)`.
- Use SQLAlchemy `JSON` for `department_path_json`.
- Add `user_id` nullable foreign key to `UserToken`.
- Add `plain_token` nullable string to `UserToken`.
- Add `AuthFlowState` and `PortalSession`.

In `backend/app/config.py`, add:

```python
auth_mode: str = "wecom"
wecom_corp_id: str = ""
wecom_agent_id: str = ""
wecom_corp_secret: str = ""
wecom_redirect_uri: str = ""
portal_session_secret: str = "dev-portal-session-secret-change-me"
portal_session_days: int = 30
```

In `backend/app/security.py`, add:

```python
import hmac
import secrets

def generate_api_token() -> str:
    return f"tk_{secrets.token_urlsafe(32)}"

def token_hint(token: str) -> str:
    return f"{token[:4]}...{token[-4:]}"

def hash_secret(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()

def sign_value(value: str, secret: str) -> str:
    sig = hmac.new(secret.encode("utf-8"), value.encode("utf-8"), hashlib.sha256).hexdigest()
    return f"{value}.{sig}"

def unsign_value(signed: str, secret: str) -> str | None:
    try:
        value, sig = signed.rsplit(".", 1)
    except ValueError:
        return None
    expected = hmac.new(secret.encode("utf-8"), value.encode("utf-8"), hashlib.sha256).hexdigest()
    if not hmac.compare_digest(sig, expected):
        return None
    return value
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd backend && uv run pytest tests/test_portal_auth.py -q`

Expected: PASS.

- [ ] **Step 5: Run existing submit tests**

Run: `cd backend && uv run pytest tests/test_submit.py -q`

Expected: PASS, proving existing bearer-token auth still works with nullable new fields.

- [ ] **Step 6: Commit**

```bash
git add backend/app/models.py backend/app/config.py backend/app/security.py backend/tests/test_portal_auth.py
git commit -m "feat(portal): add user session data model"
```

## Task 2: Implement Auth Flow State And WeCom Client

**Files:**
- Create: `backend/app/services/__init__.py`
- Create: `backend/app/services/auth_flow.py`
- Create: `backend/app/services/wecom.py`
- Test: `backend/tests/test_portal_auth.py`
- Modify: `backend/pyproject.toml`

- [ ] **Step 1: Write failing tests for state and WeCom URL generation**

Append tests to `backend/tests/test_portal_auth.py`:

```python
async def test_create_and_consume_state_once(client):
    async with db.SessionLocal() as session:
        state = await auth_flow.create_state(session, provider="wecom", entry="qr", return_to="/dashboard")
        row = await auth_flow.consume_state(session, state)
        assert row.provider == "wecom"
        assert row.return_to == "/dashboard"
        with pytest.raises(HTTPException):
            await auth_flow.consume_state(session, state)
```

```python
def test_normalize_return_to_rejects_unsafe_paths():
    assert auth_flow.normalize_return_to(None) == "/dashboard"
    assert auth_flow.normalize_return_to("/dashboard?view=year") == "/dashboard?view=year"
    with pytest.raises(HTTPException):
        auth_flow.normalize_return_to("https://evil.example")
    with pytest.raises(HTTPException):
        auth_flow.normalize_return_to("/api/auth/wecom/callback")
```

```python
def test_wecom_build_login_url(monkeypatch):
    monkeypatch.setattr(config.settings, "wecom_corp_id", "corp")
    monkeypatch.setattr(config.settings, "wecom_agent_id", "100001")
    monkeypatch.setattr(config.settings, "wecom_corp_secret", "secret")
    monkeypatch.setattr(config.settings, "wecom_redirect_uri", "https://tokusage.example/api/auth/wecom/callback")
    url = wecom.WeComClient().build_login_url(entry="qr", state="state1")
    assert "open.work.weixin.qq.com/wwopen/sso/qrConnect" in url
    assert "appid=corp" in url
    assert "agentid=100001" in url
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && uv run pytest tests/test_portal_auth.py -q`

Expected: FAIL because service modules do not exist and `httpx` is not a runtime dependency.

- [ ] **Step 3: Add dependencies**

Modify `backend/pyproject.toml` dependencies:

```toml
"httpx>=0.27",
"jinja2>=3.1",
```

Move or duplicate `httpx` from dev dependencies only if needed; it must be in runtime dependencies because `WeComClient` uses it in production.

- [ ] **Step 4: Implement `auth_flow.py`**

Port the safe shape from `zy-research`, adapted for `/dashboard`:

- `STATE_TTL_MINUTES = 10`
- `normalize_return_to()` allows `/dashboard` and `/dashboard?...` only.
- `create_state(session, provider, entry, return_to)` stores `hash_secret(state)`.
- `consume_state(session, state)` locks/loads row, rejects missing/expired/consumed, sets `consumed_at`, commits.

- [ ] **Step 5: Implement `wecom.py`**

Implement:

- `validate_config()`
- `build_login_url(entry, state)` for `qr` and `oauth`
- cached `access_token()`
- `get_userinfo(code)`
- `get_user(userid)`
- `get_department_list()`
- `resolve_department_path(member_profile)`

Keep HTTP error details safe: do not log or return access tokens or secrets.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cd backend && uv run pytest tests/test_portal_auth.py -q`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add backend/pyproject.toml backend/app/services/__init__.py backend/app/services/auth_flow.py backend/app/services/wecom.py backend/tests/test_portal_auth.py
git commit -m "feat(portal): add wecom auth flow services"
```

## Task 3: Implement Portal Users, Token Creation, And Sessions

**Files:**
- Create: `backend/app/services/portal_users.py`
- Create: `backend/app/services/portal_sessions.py`
- Test: `backend/tests/test_portal_auth.py`

- [ ] **Step 1: Write failing tests for user login and token creation**

Append tests:

```python
async def test_login_wecom_user_creates_user_department_and_token(client):
    profile = {
        "name": "Alice",
        "avatar_url": "https://example.com/a.png",
        "department_path": ["公司", "平台部", "AI 工程"],
    }
    async with db.SessionLocal() as session:
        user = await portal_users.login_wecom_user(
            session,
            corp_id="corp",
            userid="alice",
            profile=profile,
        )
        token = await session.scalar(select(models.UserToken).where(models.UserToken.user_id == user.id))

    assert user.name == "Alice"
    assert user.department_path_json == ["公司", "平台部", "AI 工程"]
    assert user.secondary_department == "平台部"
    assert token is not None
    assert token.plain_token.startswith("tk_")
    assert token.team_id == "平台部"
    assert token.user_label == "Alice"
```

```python
async def test_login_wecom_user_reuses_existing_active_token(client):
    async with db.SessionLocal() as session:
        first = await portal_users.login_wecom_user(session, corp_id="corp", userid="alice", profile={"name": "Alice"})
        second = await portal_users.login_wecom_user(session, corp_id="corp", userid="alice", profile={"name": "Alice B"})
        tokens = (await session.execute(select(models.UserToken).where(models.UserToken.user_id == first.id))).scalars().all()
    assert first.id == second.id
    assert len(tokens) == 1
```

```python
async def test_create_and_require_portal_session(client):
    async with db.SessionLocal() as session:
        user = models.PortalUser(wecom_corp_id="corp", wecom_userid="alice", name="Alice", status="active")
        session.add(user)
        await session.commit()
        await session.refresh(user)
        signed = await portal_sessions.create_session(session, user)
        loaded = await portal_sessions.load_user_from_signed_session(session, signed)
    assert loaded.id == user.id
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && uv run pytest tests/test_portal_auth.py -q`

Expected: FAIL because portal user/session services do not exist.

- [ ] **Step 3: Implement `portal_users.py`**

Implement:

- `derive_secondary_department(path: list[str] | None) -> str`
- `login_wecom_user(session, corp_id, userid, profile) -> PortalUser`
- `ensure_active_token(session, user) -> UserToken`

Rules:

- Key user by `wecom_corp_id + wecom_userid`.
- Store `department_path_json` from `profile["department_path"]`, default `[]`.
- `secondary_department` is second path segment, first segment if only one,
  empty string if no path.
- `team_id` on generated token should use `secondary_department` if present,
  otherwise `"unknown"`.
- `user_label` should use user name.
- Generate `plain_token`, `token_hash`, and `token_hint`.
- Do not create duplicate active tokens for an existing user.

- [ ] **Step 4: Implement `portal_sessions.py`**

Implement:

- session cookie name constant: `tokusage_session`
- `create_session(session, user) -> signed_cookie_value`
- `load_user_from_signed_session(session, signed_value) -> PortalUser | None`
- `revoke_session(session, signed_value) -> None`

Use `security.sign_value()` / `security.unsign_value()` and store only
`hash_secret(raw_session_secret)` in the DB.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd backend && uv run pytest tests/test_portal_auth.py -q`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add backend/app/services/portal_users.py backend/app/services/portal_sessions.py backend/tests/test_portal_auth.py
git commit -m "feat(portal): create wecom users and sessions"
```

## Task 4: Add Portal Auth Routes

**Files:**
- Create: `backend/app/routers/__init__.py`
- Create: `backend/app/routers/auth.py`
- Modify: `backend/app/main.py`
- Test: `backend/tests/test_portal_auth.py`

- [ ] **Step 1: Write failing route tests**

Append tests:

```python
async def test_wecom_login_url_route_returns_url(client, monkeypatch):
    monkeypatch.setattr(config.settings, "wecom_corp_id", "corp")
    monkeypatch.setattr(config.settings, "wecom_agent_id", "100001")
    monkeypatch.setattr(config.settings, "wecom_corp_secret", "secret")
    monkeypatch.setattr(config.settings, "wecom_redirect_uri", "https://tokusage.example/api/auth/wecom/callback")
    response = await client.get("/api/auth/wecom/login-url", params={"entry": "qr", "return_to": "/dashboard"})
    assert response.status_code == 200
    assert response.json()["url"].startswith("https://open.work.weixin.qq.com/")
```

```python
async def test_wecom_callback_creates_session_cookie(client, monkeypatch):
    async def fake_get_userinfo(self, code):
        return {"userid": "alice"}
    async def fake_get_user(self, userid):
        return {
            "name": "Alice",
            "avatar": "https://example.com/a.png",
            "department_path": ["公司", "平台部"],
        }
    monkeypatch.setattr(wecom.WeComClient, "get_userinfo", fake_get_userinfo)
    monkeypatch.setattr(wecom.WeComClient, "get_user", fake_get_user)
    monkeypatch.setattr(wecom.WeComClient, "validate_config", lambda self: None)
    monkeypatch.setattr(config.settings, "wecom_corp_id", "corp")

    async with db.SessionLocal() as session:
        state = await auth_flow.create_state(session, provider="wecom", entry="qr", return_to="/dashboard")

    response = await client.get("/api/auth/wecom/callback", params={"code": "code1", "state": state}, follow_redirects=False)
    assert response.status_code == 307
    assert response.headers["location"] == "/dashboard"
    assert "tokusage_session=" in response.headers["set-cookie"]
```

```python
async def test_me_returns_profile_and_plain_token(client):
    # Create user + token + signed session, then call /api/me with cookie.
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && uv run pytest tests/test_portal_auth.py -q`

Expected: FAIL because routes are not registered.

- [ ] **Step 3: Implement `routers/auth.py`**

Endpoints:

- `GET /api/auth/wecom/login-url`
- `GET /api/auth/wecom/callback`
- `GET /api/me`
- `POST /api/logout`

Implementation notes:

- Use `RedirectResponse("/dashboard")` after successful callback.
- Set cookie:
  - `httponly=True`
  - `samesite="lax"`
  - `secure=False` for local/dev; optionally derive from request URL scheme.
  - max age from `settings.portal_session_days`.
- `/api/me` returns:
  - `id`, `name`, `avatar_url`
  - `secondary_department`
  - `department_path`
  - active token `plain_token`
  - `token_hint`
- `/api/logout` revokes session and clears cookie.

- [ ] **Step 4: Register router in `main.py`**

Keep existing `app.include_router(router)` for `backend/app/routes.py`.
Add:

```python
from .routers import auth as portal_auth

app.include_router(portal_auth.router)
```

- [ ] **Step 5: Run route tests**

Run: `cd backend && uv run pytest tests/test_portal_auth.py -q`

Expected: PASS.

- [ ] **Step 6: Run existing backend tests**

Run: `cd backend && uv run pytest -q`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add backend/app/routers/__init__.py backend/app/routers/auth.py backend/app/main.py backend/tests/test_portal_auth.py
git commit -m "feat(portal): add wecom auth routes"
```

## Task 5: Implement Dashboard Query Service

**Files:**
- Create: `backend/app/services/dashboard.py`
- Test: `backend/tests/test_dashboard_api.py`

- [ ] **Step 1: Write failing service tests with seeded raw events**

Create `backend/tests/test_dashboard_api.py` with SQLite setup and seed:

- One portal user.
- One active token linked by `user_id`.
- Raw events across multiple days, weeks, sources, and models.

Test cases:

```python
async def test_dashboard_overview_calculates_token_metrics(client):
    async with db.SessionLocal() as session:
        user = await seed_dashboard_user(session)
        overview = await dashboard.fetch_overview(session, user, year=2026, month=6)

    assert overview["total_tokens"] == 1000
    assert overview["most_used_model"]["model"] == "claude-opus-4.7"
    assert overview["peak_day"]["date"] == "2026-06-10"
    assert overview["active_days"] == 3
    assert overview["current_streak_days"] == 2
    assert overview["longest_streak_days"] == 2
```

```python
async def test_dashboard_calendar_returns_zero_filled_month_days(client):
    async with db.SessionLocal() as session:
        user = await seed_dashboard_user(session)
        rows = await dashboard.fetch_calendar(session, user, view="month", year=2026, month=6)

    assert rows[0]["date"] == "2026-06-01"
    assert rows[-1]["date"] == "2026-06-30"
    assert any(row["total_tokens"] > 0 for row in rows)
```

```python
async def test_dashboard_day_detail_groups_by_source_model_provider(client):
    async with db.SessionLocal() as session:
        user = await seed_dashboard_user(session)
        detail = await dashboard.fetch_day_detail(session, user, day=dt.date(2026, 6, 10))

    assert detail["total_tokens"] > 0
    assert detail["breakdown"]["input_tokens"] > 0
    assert detail["models"][0]["source"] == "claude"
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && uv run pytest tests/test_dashboard_api.py -q`

Expected: FAIL because dashboard service does not exist.

- [ ] **Step 3: Implement dashboard aggregation helpers**

In `backend/app/services/dashboard.py`, implement:

- `token_total_expr()` for SQL sums.
- `fetch_daily_totals(session, user, date_from, date_to)`.
- `fetch_overview(session, user, year, month)`.
- `fetch_calendar(session, user, view, year, month)`.
- `fetch_day_detail(session, user, day)`.

Calculation rules from the spec:

- Total tokens = input + output + cache read + cache write + reasoning.
- Event count = `COUNT(raw_usage_events.id)`.
- Active day = daily total tokens > 0.
- Peak week starts Monday.
- Most-used model groups by `model`.
- Query only events where `UserToken.user_id == current_user.id`.

Use Python for streak and zero-fill date calculations after fetching grouped
daily rows; keep SQL focused on aggregation.

- [ ] **Step 4: Run dashboard service tests**

Run: `cd backend && uv run pytest tests/test_dashboard_api.py -q`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/app/services/dashboard.py backend/tests/test_dashboard_api.py
git commit -m "feat(portal): add dashboard aggregations"
```

## Task 6: Add Dashboard API Routes

**Files:**
- Create: `backend/app/routers/dashboard.py`
- Modify: `backend/app/main.py`
- Test: `backend/tests/test_dashboard_api.py`

- [ ] **Step 1: Write failing route tests**

Append route tests:

```python
async def test_dashboard_routes_require_session(client):
    response = await client.get("/api/dashboard/overview", params={"year": 2026, "month": 6})
    assert response.status_code == 401
```

```python
async def test_dashboard_overview_route_returns_current_user_metrics(client):
    cookie = await seed_user_token_events_and_session()
    response = await client.get(
        "/api/dashboard/overview",
        params={"year": 2026, "month": 6},
        cookies={"tokusage_session": cookie},
    )
    assert response.status_code == 200
    assert response.json()["total_tokens"] > 0
```

Add equivalent tests for `/api/dashboard/calendar` and `/api/dashboard/day-detail`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && uv run pytest tests/test_dashboard_api.py -q`

Expected: FAIL because routes do not exist.

- [ ] **Step 3: Implement `routers/dashboard.py`**

Add:

- `require_portal_user` dependency that reads `tokusage_session` cookie and uses
  `portal_sessions.load_user_from_signed_session`.
- `GET /api/dashboard/overview`
- `GET /api/dashboard/calendar`
- `GET /api/dashboard/day-detail`

Validate:

- `view` is `month` or `year`.
- `month` is required for month view and must be 1-12.
- date parsing errors return `422`.

- [ ] **Step 4: Register dashboard router**

In `backend/app/main.py`:

```python
from .routers import dashboard as portal_dashboard

app.include_router(portal_dashboard.router)
```

- [ ] **Step 5: Run dashboard API tests**

Run: `cd backend && uv run pytest tests/test_dashboard_api.py -q`

Expected: PASS.

- [ ] **Step 6: Run full backend tests**

Run: `cd backend && uv run pytest -q`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add backend/app/routers/dashboard.py backend/app/main.py backend/tests/test_dashboard_api.py
git commit -m "feat(portal): expose dashboard api"
```

## Task 7: Add Server-Rendered Pages And Static Assets

**Files:**
- Create: `backend/app/routers/pages.py`
- Create: `backend/app/templates/login.html`
- Create: `backend/app/templates/dashboard.html`
- Create: `backend/app/static/dashboard.js`
- Create: `backend/app/static/portal.css`
- Modify: `backend/app/main.py`
- Test: `backend/tests/test_portal_pages.py`

- [ ] **Step 1: Write failing page tests**

Create `backend/tests/test_portal_pages.py`:

```python
async def test_login_page_renders(client):
    response = await client.get("/login")
    assert response.status_code == 200
    assert "企业微信" in response.text
```

```python
async def test_dashboard_redirects_without_session(client):
    response = await client.get("/dashboard", follow_redirects=False)
    assert response.status_code in {302, 307}
    assert response.headers["location"].startswith("/login")
```

```python
async def test_dashboard_page_renders_with_session(client):
    cookie = await seed_user_token_events_and_session()
    response = await client.get("/dashboard", cookies={"tokusage_session": cookie})
    assert response.status_code == 200
    assert "每日活跃" in response.text
    assert "data-dashboard-root" in response.text
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && uv run pytest tests/test_portal_pages.py -q`

Expected: FAIL because pages do not exist.

- [ ] **Step 3: Implement page router**

Use `Jinja2Templates(directory=...)` and FastAPI `StaticFiles`.

Routes:

- `GET /login`: render login template.
- `GET /dashboard`: require portal session; redirect to `/login` if missing.

In `backend/app/main.py`:

```python
from fastapi.staticfiles import StaticFiles
from .routers import pages

app.mount("/static", StaticFiles(directory=Path(__file__).parent / "static"), name="static")
app.include_router(pages.router)
```

- [ ] **Step 4: Implement login template**

Content:

- Minimal branded page.
- Button text: `企业微信登录`.
- Script calls `/api/auth/wecom/login-url?entry=qr&return_to=/dashboard` and redirects to returned URL.
- If user agent contains `wxwork`, request `entry=oauth`.

- [ ] **Step 5: Implement dashboard template and CSS**

Follow A layout:

- Header with brand, `月/年` segmented control, `Tokens` label, avatar.
- Top stat grid.
- Main heatmap panel.
- Line chart below heatmap.
- Month summary card.
- Day detail panel.
- Avatar dropdown.

Do not include cost, session count, or 3D controls.

- [ ] **Step 6: Implement dashboard JavaScript**

In `dashboard.js`:

- Fetch `/api/me`.
- Fetch overview/calendar/day-detail for current month on load.
- Render stat cards.
- Render month/year heatmap with CSS grid.
- Render simple SVG line chart from daily totals.
- Click heatmap cell or line point to fetch day detail.
- Toggle month/year view.
- Navigate previous/next period.
- Toggle avatar menu.
- Copy token with `navigator.clipboard.writeText`.
- POST `/api/logout` and redirect to `/login`.

- [ ] **Step 7: Run page tests**

Run: `cd backend && uv run pytest tests/test_portal_pages.py -q`

Expected: PASS.

- [ ] **Step 8: Run backend test suite**

Run: `cd backend && uv run pytest -q`

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add backend/app/routers/pages.py backend/app/templates/login.html backend/app/templates/dashboard.html backend/app/static/dashboard.js backend/app/static/portal.css backend/app/main.py backend/tests/test_portal_pages.py
git commit -m "feat(portal): add dashboard pages"
```

## Task 8: Verify End-To-End Portal Behavior

**Files:**
- Modify only if verification finds defects.
- Test: existing backend tests.

- [ ] **Step 1: Run all backend tests**

Run: `cd backend && uv run pytest -q`

Expected: PASS.

- [ ] **Step 2: Run Rust tests**

Run: `cargo test`

Expected: PASS. This proves CLI/core behavior is not broken.

- [ ] **Step 3: Start backend locally**

Run:

```bash
cd backend
TOKUSAGE_DATABASE_URL=sqlite+aiosqlite:///./local-portal.sqlite3 \
TOKUSAGE_WECOM_CORP_ID=corp \
TOKUSAGE_WECOM_AGENT_ID=100001 \
TOKUSAGE_WECOM_CORP_SECRET=secret \
TOKUSAGE_WECOM_REDIRECT_URI=http://127.0.0.1:8080/api/auth/wecom/callback \
uv run uvicorn app.main:app --reload --port 8080
```

Expected: server starts. Real WeCom login will not complete with fake settings,
but `/login`, `/health`, and static assets should load.

- [ ] **Step 4: Manually inspect local pages**

Open:

- `http://127.0.0.1:8080/login`
- `http://127.0.0.1:8080/health`

For an authenticated dashboard preview, use route tests or a temporary seeded
session in test DB rather than adding a production debug bypass.

- [ ] **Step 5: Final commit if fixes were needed**

Only commit if verification required changes:

```bash
git add <changed-files>
git commit -m "fix(portal): polish dashboard verification issues"
```

## Open Implementation Notes

- The plan intentionally does not backfill ownership for existing `user_tokens`
  rows. Existing bearer tokens continue working. Portal-created tokens are
  linked to users and power the dashboard.
- Production deployments with persistent data should add explicit migration
  scripts before rollout. The current app uses `Base.metadata.create_all`, which
  creates missing tables but does not alter existing tables in place.
- Keep cost, session count, and 3D controls out of the first version.
- Do not log WeCom access tokens, corp secret, generated API tokens, or session
  cookie values.
