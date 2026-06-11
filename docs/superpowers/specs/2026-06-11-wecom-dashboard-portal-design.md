# WeCom Dashboard Portal Design

## Goal

Add a server-side web portal to `tokusage` where a user can sign in with
Enterprise WeChat, receive or view their personal API token, and inspect their
own token usage dashboard.

The portal is for authenticated personal usage. Team-wide administration,
billing, strict session analytics, and cost reporting are out of scope for this
version.

## Confirmed Scope

- Use Enterprise WeChat login, following the same broad flow as
  `/Users/gd/zy-research`.
- After first WeCom login, create a local user record and automatically create
  a personal token.
- Store the user's primary department path from WeCom. Derive the secondary
  department from the second path segment; if only one segment exists, use that;
  if unavailable, store an empty value.
- Store the plaintext API token so the signed-in user can view and copy it from
  the avatar menu. Keep `token_hash` for existing bearer-token authentication.
  Plaintext storage is intentionally accepted for this internal token.
- Build the dashboard as a FastAPI-served page using server templates and
  lightweight browser JavaScript. Do not introduce a separate Vue/Vite frontend
  in the first version.
- Use the selected "A" visual direction: a single-page analysis dashboard with
  top stat cards, a main matrix visualization, a line chart below it, and a day
  detail area.
- Do not show cost/fee metrics.
- Do not show session count.
- Do not build a 3D view.

## Current Data Audit

Current raw usage storage is sufficient for most dashboard metrics:

- `raw_usage_events.event_ts` supports day/month/year grouping, heatmap buckets,
  peak day, peak week, active day counts, streaks, and daily trend lines.
- `source`, `model`, and `provider` support breakdowns by client and model.
- `input_tokens`, `output_tokens`, `cache_read_tokens`,
  `cache_write_tokens`, and `reasoning_tokens` support total token counts and
  token detail cards.
- `cost_cents` exists, but cost is incomplete because Claude and Codex currently
  submit `0.0`; cost UI is therefore excluded.
- `session_key` exists, but it is not a stable cross-source conversation count:
  Claude has session-file keys, Codex currently stores `session + turn`, and
  Cursor has no session key. Session count UI is therefore excluded.
- Existing events are tied to `user_token_id`; after user login is added,
  `user_tokens` must link to a portal user so dashboard queries can aggregate
  all tokens owned by the signed-in user.

## Auth Design

Add lightweight portal auth alongside the existing API-token auth.

### Settings

Add environment-backed settings:

- `TOKUSAGE_AUTH_MODE`, default `wecom`
- `TOKUSAGE_WECOM_CORP_ID`
- `TOKUSAGE_WECOM_AGENT_ID`
- `TOKUSAGE_WECOM_CORP_SECRET`
- `TOKUSAGE_WECOM_REDIRECT_URI`
- `TOKUSAGE_PORTAL_SESSION_SECRET`
- `TOKUSAGE_FRONTEND_AUTH_SUCCESS_URL`, or derive it from the backend base URL
  if the portal is served by the same FastAPI app.

### WeCom Flow

1. User opens `/login`.
2. Browser requests `/api/auth/wecom/login-url?entry=qr&return_to=/dashboard`.
3. Backend validates WeCom config, creates a one-time state, and returns a WeCom
   QR or OAuth login URL.
4. WeCom redirects to `/api/auth/wecom/callback?code=...&state=...`.
5. Backend consumes state, fetches `auth/getuserinfo`, then fetches member data
   with `user/get`.
6. Backend resolves or creates the local portal user by `corp_id + userid`.
7. Backend stores latest profile fields and department path.
8. Backend creates a personal API token if the user does not already have an
   active token.
9. Backend sets an HTTP-only session cookie and redirects to `/dashboard`.

Use the same state/login-code safety shape as `zy-research`: random state,
stored as a hash, short TTL, single-use consumption, and safe `return_to`
validation. Because this portal is served by the backend, a separate frontend
exchange-code step is not required unless later split into a standalone
frontend.

### Local User Tables

Add portal user tables rather than overloading `user_tokens` as identity:

`users`

- `id`
- `wecom_corp_id`
- `wecom_userid`
- `name`
- `avatar_url`
- `department_path_json`
- `secondary_department`
- `status`
- `last_login_at`
- `created_at`
- `updated_at`

`auth_flow_states`

- `state_hash`
- `provider`
- `entry`
- `return_to`
- `expires_at`
- `consumed_at`
- `created_at`

`portal_sessions`

- `session_hash`
- `user_id`
- `expires_at`
- `revoked_at`
- `created_at`
- `last_seen_at`

`user_tokens` additions:

- `user_id`, nullable during migration for existing token rows.
- `plain_token`, nullable for legacy rows until token backfill or regeneration.

Existing bearer auth remains DB-backed: incoming `Authorization: Bearer <token>`
is hashed and matched against `user_tokens.token_hash`.

## Department Storage

WeCom member data commonly returns department IDs rather than full names. The
implementation should store the best available primary department path:

1. Read the member's primary department ID when available.
2. Resolve department names through WeCom department APIs when configured and
   available.
3. Store the resolved path as an ordered JSON array, for example
   `["公司", "平台部", "AI 工程"]`.
4. Set `secondary_department` to the second element.
5. If only one element exists, set `secondary_department` to that element.
6. If no path can be resolved, store an empty array and empty
   `secondary_department`.

Keep the full path so a later product change can derive department views
without reworking the user model.

## Token Lifecycle

On first login:

1. Generate a `tk_...` token using the existing token-generation style.
2. Store `plain_token`.
3. Store `token_hash = sha256(plain_token)`.
4. Store `token_hint`.
5. Link the row to `users.id`.
6. Mark it active.

For existing users:

- If an active token exists, show that token in the avatar menu.
- If no active token exists, create one at login.
- Legacy token rows without `plain_token` can be shown as hint-only until the
  user regenerates, or regenerated automatically when a matching portal user is
  known. The first implementation should avoid guessing ownership for legacy
  rows unless the token was created by a portal login.

## Dashboard UI

The dashboard uses the selected A layout.

### Header

- `tokusage` brand.
- View controls: `月`, `年`.
- Metric label: `Tokens`.
- Avatar button.
- Avatar dropdown:
  - user name
  - secondary department
  - full primary department path
  - full plaintext API token
  - copy token button
  - logout button

### Top Summary Cards

Keep these cards:

- Most-used model by Tokens.
- Total Tokens.
- Current streak days.
- Longest streak days.
- Active days, displayed as `active / days in selected year`.
- Peak day: date and Tokens.
- Peak week: date range and Tokens.
- Highest active weekday: weekday and Tokens.
- Active-day average Tokens.

Do not show cost or session count.

### Main Visualization

Month view:

- Daily heatmap for the selected month.
- Previous/next month navigation.
- Legend from low to high Tokens.
- Daily line chart below the heatmap for each day in the selected month.
- Clicking a heatmap cell or line point selects the day and refreshes day
  detail.

Year view:

- Daily heatmap across the selected year, grouped visually by month.
- Previous/next year navigation.
- Legend from low to high Tokens.
- Daily line chart below the heatmap for all days in the selected year.
- Clicking a day selects it and refreshes day detail.

3D view is excluded.

### Month And Year Summaries

Month summary card:

- Current month total Tokens.
- Current month message/event count.
- Current month streak days.
- Current month active-day average Tokens.
- Current month most-used model by Tokens.
- Current month active days.

Year/month summary table:

- Month.
- Total Tokens.
- Event count.
- Active days.
- Most-used model by Tokens.

These extra columns are retained unless later removed during implementation
review.

### Day Detail

When a day is selected, show:

- Date.
- Total Tokens.
- Token detail:
  - input
  - output
  - cache read
  - cache write
  - reasoning
- Model usage table:
  - source
  - model
  - total Tokens
  - input/output/cache/reasoning detail columns

## Dashboard API Design

All dashboard endpoints use portal session auth, not bearer-token auth.

`GET /api/me`

Returns signed-in user profile and active token.

`GET /api/dashboard/overview?year=2026&month=6`

Returns top summary cards for the selected scope and high-level month/year
summaries.

`GET /api/dashboard/calendar?view=month&year=2026&month=6`

Returns one row per date:

- date
- total tokens
- event count
- input/output/cache/reasoning sums

`GET /api/dashboard/day-detail?date=2026-06-10`

Returns day totals and rows grouped by source/model/provider.

The backend should implement dashboard queries against `raw_usage_events`
joined through `user_tokens.user_id == current_user.id`.

## Data Semantics

Token total:

```text
input_tokens
+ output_tokens
+ cache_read_tokens
+ cache_write_tokens
+ reasoning_tokens
```

Event count:

```text
COUNT(raw_usage_events.id)
```

Active day:

```text
daily total tokens > 0
```

Streak:

- Consecutive active days ending at the latest active day in the selected scope.
- Longest streak is the maximum consecutive active-day run in the selected
  scope.

Peak week:

- Use calendar weeks starting Monday.
- Sum Tokens over each week.
- Display the inclusive date range.

Highest active weekday:

- Group active dates by weekday.
- Sum Tokens by weekday.
- Display the highest total.

Most-used model:

- Group by model.
- Sum Tokens.
- Highest sum wins.
- No cost ranking in this version.

## Error Handling

- Missing WeCom config returns a server-side configuration error.
- Invalid or expired state returns a login failure page with a retry action.
- Non-enterprise WeCom users are rejected.
- Disabled local users cannot log in.
- Missing session redirects to `/login`.
- Dashboard endpoints return `401` for missing/expired portal sessions.
- Bearer-token API auth continues returning `401` for missing or invalid API
  tokens.

## Testing

Backend tests:

- WeCom login URL creation validates config and creates state.
- WeCom callback consumes state, fetches user info, creates user, stores
  department path, derives secondary department, creates token, and sets
  session.
- Existing WeCom user login updates profile and does not create duplicate active
  tokens.
- Avatar token endpoint returns the linked plaintext token.
- Dashboard overview calculates token totals, streaks, peak day/week, active
  day average, and most-used model.
- Calendar endpoint returns month and year daily rows.
- Day detail endpoint groups by source/model/provider and includes token
  breakdown.
- Bearer-token submit remains compatible with `user_tokens.token_hash`.

UI tests can be lightweight because the first version is server-rendered:

- Login page redirects to WeCom login URL.
- Dashboard renders summary cards, month/year toggle, heatmap, line chart, day
  detail, and avatar dropdown.
- Clicking a date updates day detail.
- Copy token button writes the token to the clipboard where browser test support
  allows it.

## Migration Notes

The app currently initializes schema with SQLAlchemy metadata. This design can
be implemented with metadata updates in the short term. If production data is
already persistent, add explicit migration scripts before deployment so new
columns and tables are added without dropping data.

Existing token rows will have `user_id = NULL` and `plain_token = NULL`. They
continue to authenticate CLI submits. Portal-created token rows should always
have both fields populated.
