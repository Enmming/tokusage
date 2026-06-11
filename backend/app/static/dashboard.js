const state = {
  view: "month",
  year: new Date().getFullYear(),
  month: new Date().getMonth() + 1,
  selectedDate: null,
  calendar: [],
  profile: null,
};

const fmt = new Intl.NumberFormat("en-US");

function $(selector) {
  return document.querySelector(selector);
}

function formatTokens(value) {
  return fmt.format(Math.round(value || 0));
}

function periodParams() {
  const params = new URLSearchParams({ year: String(state.year) });
  if (state.view === "month") params.set("month", String(state.month));
  return params;
}

async function fetchJson(url) {
  const response = await fetch(url);
  if (response.status === 401) {
    window.location.assign("/login");
    return null;
  }
  if (!response.ok) throw new Error(`Request failed: ${response.status}`);
  return response.json();
}

async function loadProfile() {
  state.profile = await fetchJson("/api/me");
  if (!state.profile) return;
  $("#profile-name").textContent = state.profile.name;
  $("#profile-department").textContent = state.profile.secondary_department || "未记录部门";
  $("#profile-path").textContent = (state.profile.department_path || []).join(" / ");
  $("#profile-token").textContent = state.profile.plain_token || state.profile.token_hint || "";
}

async function loadDashboard() {
  const overview = await fetchJson(`/api/dashboard/overview?${periodParams().toString()}`);
  if (!overview) return;
  const calendarParams = new URLSearchParams({
    view: state.view,
    year: String(state.year),
  });
  if (state.view === "month") calendarParams.set("month", String(state.month));
  state.calendar = await fetchJson(`/api/dashboard/calendar?${calendarParams.toString()}`);
  renderPeriodTitle();
  renderStats(overview);
  renderMonthSummary(overview);
  renderHeatmap();
  renderLineChart();
  renderMonthTable();
  const selected = state.selectedDate || latestActiveDate() || state.calendar[0]?.date;
  if (selected) await selectDate(selected);
}

function renderPeriodTitle() {
  $("#period-title").textContent =
    state.view === "month" ? `${state.year}年${state.month}月` : `${state.year}`;
}

function renderStats(overview) {
  const cards = [
    ["最常用模型", overview.most_used_model?.model || "-"],
    ["总 Token 数", formatTokens(overview.total_tokens)],
    ["连续天数", `${overview.current_streak_days || 0} 天`],
    ["最长连续天数", `${overview.longest_streak_days || 0} 天`],
    ["活跃天数", `${overview.active_days || 0}/${overview.days_in_year || 365}`],
    ["峰值日", overview.peak_day ? overview.peak_day.date : "-", overview.peak_day ? `Tokens: ${formatTokens(overview.peak_day.total_tokens)}` : ""],
    ["峰值周", overview.peak_week ? `${overview.peak_week.date_from} → ${overview.peak_week.date_to}` : "-", overview.peak_week ? `Tokens: ${formatTokens(overview.peak_week.total_tokens)}` : ""],
    ["活跃日均值", formatTokens(overview.active_day_average_tokens)],
    ["最高活跃日", overview.highest_active_weekday ? overview.highest_active_weekday.weekday : "-", overview.highest_active_weekday ? `Tokens: ${formatTokens(overview.highest_active_weekday.total_tokens)}` : ""],
  ];
  $("#stats-grid").innerHTML = cards.map(([label, value, note]) => `
    <article class="stat-card">
      <span>${label}</span>
      <strong>${value}</strong>
      ${note ? `<small>${note}</small>` : ""}
    </article>
  `).join("");
}

function renderMonthSummary(overview) {
  const items = [
    ["总 Token 数", formatTokens(overview.total_tokens)],
    ["事件数", formatTokens(overview.event_count)],
    ["连续天数", `${overview.current_streak_days || 0} 天`],
    ["活跃日均值", formatTokens(overview.active_day_average_tokens)],
    ["最常用模型", overview.most_used_model?.model || "-"],
    ["活跃天数", `${overview.active_days || 0}`],
  ];
  $("#month-summary").innerHTML = items.map(([label, value]) => `
    <div class="mini-stat"><span>${label}</span><strong>${value}</strong></div>
  `).join("");
}

function levelFor(row) {
  const max = Math.max(...state.calendar.map((item) => item.total_tokens || 0), 0);
  if (!row.total_tokens || !max) return 0;
  return Math.max(1, Math.ceil((row.total_tokens / max) * 5));
}

function renderHeatmap() {
  $("#legend-scale").innerHTML = Array.from({ length: 6 }, () => "<span></span>").join("");
  $("#heatmap").innerHTML = state.calendar.map((row) => `
    <button
      type="button"
      class="heat-cell${row.date === state.selectedDate ? " selected" : ""}"
      data-date="${row.date}"
      data-level="${levelFor(row)}"
      title="${row.date}: ${formatTokens(row.total_tokens)}"
      aria-label="${row.date} ${formatTokens(row.total_tokens)} Tokens"
    ></button>
  `).join("");
  document.querySelectorAll(".heat-cell").forEach((cell) => {
    cell.addEventListener("click", () => selectDate(cell.dataset.date));
  });
}

function renderLineChart() {
  const width = 800;
  const height = 150;
  const max = Math.max(...state.calendar.map((item) => item.total_tokens || 0), 1);
  const points = state.calendar.map((row, index) => {
    const x = state.calendar.length === 1 ? 0 : (index / (state.calendar.length - 1)) * width;
    const y = height - ((row.total_tokens || 0) / max) * (height - 16) - 8;
    return { x, y, row };
  });
  const path = points.map((point, index) => `${index === 0 ? "M" : "L"}${point.x},${point.y}`).join(" ");
  $("#line-chart").innerHTML = `
    <svg viewBox="0 0 ${width} ${height}" preserveAspectRatio="none">
      <path d="${path}" fill="none" stroke="#0f9f9a" stroke-width="4" vector-effect="non-scaling-stroke"></path>
      ${points.map((point) => `<circle cx="${point.x}" cy="${point.y}" r="5" fill="#2448d8" data-date="${point.row.date}"></circle>`).join("")}
    </svg>
  `;
  document.querySelectorAll("#line-chart circle").forEach((point) => {
    point.addEventListener("click", () => selectDate(point.dataset.date));
  });
}

function renderMonthTable() {
  if (state.view !== "year") {
    $("#month-table").innerHTML = "";
    return;
  }
  const byMonth = new Map();
  for (const row of state.calendar) {
    const month = Number(row.date.slice(5, 7));
    const current = byMonth.get(month) || { total: 0, events: 0, active: 0 };
    current.total += row.total_tokens || 0;
    current.events += row.event_count || 0;
    if (row.total_tokens > 0) current.active += 1;
    byMonth.set(month, current);
  }
  $("#month-table").innerHTML = Array.from({ length: 12 }, (_, index) => {
    const month = index + 1;
    const row = byMonth.get(month) || { total: 0, events: 0, active: 0 };
    return `<tr><td>${month}月</td><td>${formatTokens(row.total)}</td><td>${formatTokens(row.events)}</td><td>${row.active}</td><td>-</td></tr>`;
  }).join("");
}

function latestActiveDate() {
  const active = state.calendar.filter((row) => row.total_tokens > 0);
  return active.length ? active[active.length - 1].date : null;
}

async function selectDate(date) {
  state.selectedDate = date;
  renderHeatmap();
  const detail = await fetchJson(`/api/dashboard/day-detail?date=${encodeURIComponent(date)}`);
  if (!detail) return;
  $("#day-title").textContent = `${detail.date} 日详情`;
  const b = detail.breakdown;
  $("#day-breakdown").innerHTML = `
    <div class="breakdown-grid">
      <div class="mini-stat"><span>Tokens</span><strong>${formatTokens(detail.total_tokens)}</strong></div>
      <div class="mini-stat"><span>输入</span><strong>${formatTokens(b.input_tokens)}</strong></div>
      <div class="mini-stat"><span>输出</span><strong>${formatTokens(b.output_tokens)}</strong></div>
      <div class="mini-stat"><span>缓存读取</span><strong>${formatTokens(b.cache_read_tokens)}</strong></div>
      <div class="mini-stat"><span>缓存写入</span><strong>${formatTokens(b.cache_write_tokens)}</strong></div>
      <div class="mini-stat"><span>推理</span><strong>${formatTokens(b.reasoning_tokens)}</strong></div>
    </div>
  `;
  $("#day-models").innerHTML = (detail.models || []).map((row) => `
    <tr>
      <td>${row.source}</td>
      <td>${row.model}</td>
      <td>${formatTokens(row.total_tokens)}</td>
    </tr>
  `).join("");
}

function shiftPeriod(delta) {
  if (state.view === "year") {
    state.year += delta;
  } else {
    state.month += delta;
    if (state.month < 1) {
      state.month = 12;
      state.year -= 1;
    }
    if (state.month > 12) {
      state.month = 1;
      state.year += 1;
    }
  }
  state.selectedDate = null;
  loadDashboard();
}

function setAccountMenuOpen(open) {
  const menu = $("#account-menu");
  const button = $("#avatar-button");
  menu.hidden = !open;
  button.setAttribute("aria-expanded", String(open));
}

function bindControls() {
  document.querySelectorAll(".segment[data-view]").forEach((button) => {
    button.addEventListener("click", () => {
      state.view = button.dataset.view;
      document.querySelectorAll(".segment[data-view]").forEach((item) => {
        item.classList.toggle("active", item === button);
      });
      state.selectedDate = null;
      loadDashboard();
    });
  });
  $("#previous-period").addEventListener("click", () => shiftPeriod(-1));
  $("#next-period").addEventListener("click", () => shiftPeriod(1));
  $("#avatar-button").setAttribute("aria-controls", "account-menu");
  $("#avatar-button").setAttribute("aria-expanded", "false");
  $("#avatar-button").addEventListener("click", (event) => {
    event.stopPropagation();
    const menu = $("#account-menu");
    setAccountMenuOpen(menu.hidden);
  });
  $("#account-menu").addEventListener("click", (event) => {
    event.stopPropagation();
  });
  document.addEventListener("click", () => {
    setAccountMenuOpen(false);
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") setAccountMenuOpen(false);
  });
  $("#copy-token").addEventListener("click", async () => {
    const token = state.profile?.plain_token || "";
    if (token) await navigator.clipboard.writeText(token);
  });
  $("#logout").addEventListener("click", async () => {
    await fetch("/api/logout", { method: "POST" });
    window.location.assign("/login");
  });
}

bindControls();
loadProfile().then(loadDashboard);
