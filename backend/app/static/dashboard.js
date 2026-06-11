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

function localDateIso(date = new Date()) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function visibleCalendarRows() {
  const today = localDateIso();
  return state.calendar.filter((row) => row.date <= today);
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
  renderSummaryTable();
  const visibleRows = visibleCalendarRows();
  const selectedIsVisible = visibleRows.some((row) => row.date === state.selectedDate);
  const selected = selectedIsVisible
    ? state.selectedDate
    : latestActiveDate(visibleRows) || visibleRows[0]?.date;
  if (selected) {
    await selectDate(selected);
  } else {
    clearDayDetail();
  }
}

function renderPeriodTitle() {
  $("#period-title").textContent =
    state.view === "month" ? `${state.year}年${state.month}月` : `${state.year}`;
  $("#period-summary-title").textContent =
    state.view === "month" ? "月度统计" : "年度统计";
  $("#line-chart-title").textContent =
    state.view === "month" ? "日曲线" : "月曲线";
  $("#summary-table-title").textContent = "每日汇总";
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

function levelFor(row, rows) {
  const max = Math.max(...rows.map((item) => item.total_tokens || 0), 0);
  if (!row.total_tokens || !max) return 0;
  return Math.max(1, Math.ceil((row.total_tokens / max) * 5));
}

function renderHeatmap() {
  const rows = visibleCalendarRows();
  $("#legend-scale").innerHTML = Array.from({ length: 6 }, () => "<span></span>").join("");
  if (!rows.length) {
    $("#heatmap").innerHTML = `<div class="empty-state">当前周期暂无可显示日期</div>`;
    return;
  }
  $("#heatmap").innerHTML = rows.map((row) => `
    <button
      type="button"
      class="heat-cell${row.date === state.selectedDate ? " selected" : ""}"
      data-date="${row.date}"
      data-level="${levelFor(row, rows)}"
      title="${row.date}: ${formatTokens(row.total_tokens)}"
      aria-label="${row.date} ${formatTokens(row.total_tokens)} Tokens"
    ></button>
  `).join("");
  document.querySelectorAll(".heat-cell").forEach((cell) => {
    cell.addEventListener("click", () => selectDate(cell.dataset.date));
  });
}

function chartRows() {
  const rows = visibleCalendarRows();
  if (state.view === "month") {
    return rows.map((row) => ({
      date: row.date,
      label: String(Number(row.date.slice(8, 10))),
      total_tokens: row.total_tokens || 0,
    }));
  }

  const byMonth = new Map();
  for (const row of rows) {
    const month = Number(row.date.slice(5, 7));
    const current = byMonth.get(month) || {
      month,
      date: row.date,
      label: `${month}月`,
      total_tokens: 0,
      activeDate: null,
    };
    current.total_tokens += row.total_tokens || 0;
    current.date = row.date;
    if (row.total_tokens > 0) current.activeDate = row.date;
    byMonth.set(month, current);
  }
  return Array.from(byMonth.values()).map((row) => ({
    date: row.activeDate || row.date,
    label: row.label,
    total_tokens: row.total_tokens,
  }));
}

function tickIndexes(length) {
  if (length <= 1) return [0];
  const maxTicks = state.view === "year" ? 12 : 7;
  const step = Math.max(1, Math.ceil(length / maxTicks));
  const indexes = [];
  for (let index = 0; index < length; index += step) indexes.push(index);
  if (indexes[indexes.length - 1] !== length - 1) indexes.push(length - 1);
  return indexes;
}

function renderLineChart() {
  const width = 800;
  const height = 190;
  const plotTop = 10;
  const plotBottom = 146;
  const labelY = 176;
  const rows = chartRows();
  if (!rows.length) {
    $("#line-chart").innerHTML = `<div class="empty-state">当前周期暂无可显示日期</div>`;
    return;
  }
  const max = Math.max(...rows.map((item) => item.total_tokens || 0), 1);
  const points = rows.map((row, index) => {
    const x = rows.length === 1 ? width / 2 : (index / (rows.length - 1)) * width;
    const y = plotBottom - ((row.total_tokens || 0) / max) * (plotBottom - plotTop);
    return { x, y, row };
  });
  const path = points.map((point, index) => `${index === 0 ? "M" : "L"}${point.x},${point.y}`).join(" ");
  const ticks = tickIndexes(points.length);
  $("#line-chart").innerHTML = `
    <svg viewBox="0 0 ${width} ${height}" preserveAspectRatio="none">
      <line x1="0" y1="${plotBottom}" x2="${width}" y2="${plotBottom}" class="chart-axis"></line>
      ${ticks.map((index) => {
        const point = points[index];
        return `
          <line x1="${point.x}" y1="${plotBottom}" x2="${point.x}" y2="${plotBottom + 5}" class="chart-tick"></line>
          <text x="${point.x}" y="${labelY}" text-anchor="middle" class="chart-label">${point.row.label}</text>
        `;
      }).join("")}
      <path d="${path}" fill="none" stroke="#0f9f9a" stroke-width="4" vector-effect="non-scaling-stroke"></path>
      ${points.map((point) => `<circle cx="${point.x}" cy="${point.y}" r="5" fill="#2448d8" data-date="${point.row.date}"><title>${point.row.label}: ${formatTokens(point.row.total_tokens)}</title></circle>`).join("")}
    </svg>
  `;
  document.querySelectorAll("#line-chart circle").forEach((point) => {
    point.addEventListener("click", () => selectDate(point.dataset.date));
  });
}

function renderSummaryTable() {
  const rows = visibleCalendarRows();
  if (!rows.length) {
    $("#summary-table").innerHTML = `
      <tr><td colspan="6" class="empty-cell">当前周期暂无可显示日期</td></tr>
    `;
    return;
  }
  $("#summary-table").innerHTML = rows.map((row) => {
    return `
      <tr>
        <td>${row.date}</td>
        <td>${formatTokens(row.total_tokens)}</td>
        <td>${formatTokens(row.event_count)}</td>
        <td>${formatTokens(row.input_tokens)}</td>
        <td>${formatTokens(row.output_tokens)}</td>
        <td>${formatTokens(row.cache_read_tokens)}</td>
      </tr>
    `;
  }).join("");
}

function latestActiveDate(rows = visibleCalendarRows()) {
  const active = rows.filter((row) => row.total_tokens > 0);
  return active.length ? active[active.length - 1].date : null;
}

function clearDayDetail() {
  state.selectedDate = null;
  $("#day-title").textContent = "日详情";
  $("#day-breakdown").innerHTML = `<div class="empty-state">选择日期后查看详情</div>`;
  $("#day-models").innerHTML = "";
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
