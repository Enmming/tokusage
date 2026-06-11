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
  const millions = (value || 0) / 1_000_000;
  const fractionDigits = millions >= 10 ? 1 : 3;
  return `${fmt.format(Number(millions.toFixed(fractionDigits)))}M`;
}

function formatCount(value) {
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
  await renderPeriodModels();
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
  $("#period-models-title").textContent =
    state.view === "month" ? "本月模型统计" : "本年模型统计";
  $("#summary-table-title").textContent = "每日汇总";
}

function renderStats(overview) {
  const periodDays = overview.period_days || overview.days_in_year || 365;
  const cards = [
    ["总 Token 数 (M)", formatTokens(overview.total_tokens)],
    ["最常用模型", overview.most_used_model?.model || "-"],
    ["活跃天数", `${overview.active_days || 0}/${periodDays}`],
    ["连续天数", `${overview.current_streak_days || 0} 天`],
    ["峰值日", overview.peak_day ? overview.peak_day.date : "-", overview.peak_day ? `Tokens: ${formatTokens(overview.peak_day.total_tokens)}` : ""],
    ["活跃日均值 (M)", formatTokens(overview.active_day_average_tokens)],
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
    ["总 Token 数 (M)", formatTokens(overview.total_tokens)],
    ["事件数", formatCount(overview.event_count)],
    ["活跃天数", `${overview.active_days || 0}`],
    ["连续天数", `${overview.current_streak_days || 0} 天`],
    ["最长连续", `${overview.longest_streak_days || 0} 天`],
    ["峰值日", overview.peak_day ? overview.peak_day.date : "-"],
    ["峰值周", overview.peak_week ? `${overview.peak_week.date_from} → ${overview.peak_week.date_to}` : "-"],
    ["最高活跃星期", overview.highest_active_weekday ? overview.highest_active_weekday.weekday : "-"],
    ["最常用模型", overview.most_used_model?.model || "-"],
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

function compactTokens(value) {
  return formatTokens(value);
}

function smoothPath(points) {
  if (points.length === 1) return `M${points[0].x},${points[0].y}`;
  return points.map((point, index) => {
    if (index === 0) return `M${point.x},${point.y}`;
    const previous = points[index - 1];
    const controlOffset = (point.x - previous.x) * 0.45;
    return `C${previous.x + controlOffset},${previous.y} ${point.x - controlOffset},${point.y} ${point.x},${point.y}`;
  }).join(" ");
}

function isSelectedChartPoint(point) {
  if (!state.selectedDate) return false;
  if (state.view === "month") return point.row.date === state.selectedDate;
  return point.row.date.slice(0, 7) === state.selectedDate.slice(0, 7);
}

function chartTooltip(date, tokens) {
  return `
    <div class="chart-tooltip-title">${date}</div>
    <div class="chart-tooltip-value">${tokens} Tokens</div>
  `;
}

function renderLineChart() {
  const width = 900;
  const height = 240;
  const paddingX = 38;
  const plotTop = 28;
  const plotBottom = 178;
  const labelY = 218;
  const rows = chartRows();
  if (!rows.length) {
    $("#line-chart").innerHTML = `<div class="empty-state">当前周期暂无可显示日期</div>`;
    return;
  }
  const max = Math.max(...rows.map((item) => item.total_tokens || 0), 1);
  const points = rows.map((row, index) => {
    const x = rows.length === 1 ? width / 2 : paddingX + (index / (rows.length - 1)) * (width - paddingX * 2);
    const y = plotBottom - ((row.total_tokens || 0) / max) * (plotBottom - plotTop);
    return { x, y, row };
  });
  const path = smoothPath(points);
  const areaPath = `${path} L${points[points.length - 1].x},${plotBottom} L${points[0].x},${plotBottom} Z`;
  const ticks = tickIndexes(points.length);
  const peak = points.reduce((current, point) => (
    point.row.total_tokens > current.row.total_tokens ? point : current
  ), points[0]);
  $("#line-chart").innerHTML = `
    <svg viewBox="0 0 ${width} ${height}" preserveAspectRatio="xMidYMid meet">
      <defs>
        <linearGradient id="trend-fill" x1="0" x2="0" y1="0" y2="1">
          <stop offset="0%" stop-color="#0f9f9a" stop-opacity="0.26"></stop>
          <stop offset="74%" stop-color="#4f7ff4" stop-opacity="0.08"></stop>
          <stop offset="100%" stop-color="#4f7ff4" stop-opacity="0"></stop>
        </linearGradient>
        <linearGradient id="trend-stroke" x1="0" x2="1" y1="0" y2="0">
          <stop offset="0%" stop-color="#2448d8"></stop>
          <stop offset="58%" stop-color="#0f9f9a"></stop>
          <stop offset="100%" stop-color="#172a8a"></stop>
        </linearGradient>
      </defs>
      ${[0, 0.25, 0.5, 0.75, 1].map((ratio) => {
        const y = plotBottom - ratio * (plotBottom - plotTop);
        return `
          <line x1="${paddingX}" y1="${y}" x2="${width - paddingX}" y2="${y}" class="chart-grid"></line>
          <text x="8" y="${y + 4}" class="chart-y-label">${compactTokens(max * ratio)}</text>
        `;
      }).join("")}
      <path d="${areaPath}" class="chart-area"></path>
      <path d="${path}" class="chart-line"></path>
      ${ticks.map((index) => {
        const point = points[index];
        return `
          <line x1="${point.x}" y1="${plotBottom}" x2="${point.x}" y2="${plotBottom + 5}" class="chart-tick"></line>
          <text x="${point.x}" y="${labelY}" text-anchor="middle" class="chart-label">${point.row.label}</text>
        `;
      }).join("")}
      <g class="chart-points">
        ${points.map((point) => `
          <circle
            class="chart-point${isSelectedChartPoint(point) ? " selected" : ""}${point === peak ? " peak" : ""}"
            cx="${point.x}"
            cy="${point.y}"
            r="${point === peak || isSelectedChartPoint(point) ? 6 : 4}"
            data-date="${point.row.date}"
            data-tooltip-date="${point.row.date}"
            data-tooltip-tokens="${formatTokens(point.row.total_tokens)}"
            tabindex="0"
            role="button"
            aria-label="${point.row.date} ${formatTokens(point.row.total_tokens)} Tokens"
          ><title>${point.row.label}: ${formatTokens(point.row.total_tokens)}</title></circle>
        `).join("")}
      </g>
      <g
        class="chart-peak-label"
        transform="translate(${Math.min(width - 132, Math.max(58, peak.x - 54))}, ${Math.max(18, peak.y - 34)})"
        data-date="${peak.row.date}"
        tabindex="0"
        role="button"
        aria-label="选择峰值日期 ${peak.row.date}"
      >
        <rect width="108" height="24" rx="6"></rect>
        <text x="54" y="16" text-anchor="middle">峰值 ${compactTokens(peak.row.total_tokens)}</text>
      </g>
    </svg>
  `;
  document.querySelectorAll("#line-chart circle").forEach((point) => {
    point.addEventListener("click", () => selectDate(point.dataset.date));
    point.addEventListener("mouseenter", () => showChartTooltip(point));
    point.addEventListener("mousemove", () => showChartTooltip(point));
    point.addEventListener("mouseleave", hideChartTooltip);
    point.addEventListener("focus", () => showChartTooltip(point));
    point.addEventListener("blur", hideChartTooltip);
  });
  const peakLabel = $("#line-chart .chart-peak-label");
  peakLabel?.addEventListener("click", () => selectDate(peakLabel.dataset.date));
  peakLabel?.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      selectDate(peakLabel.dataset.date);
    }
  });
}

async function renderPeriodModels() {
  const models = await fetchJson(`/api/dashboard/period-models?${periodParams().toString()}`);
  if (!models) return;
  $("#period-models").innerHTML = models.length
    ? models.map((row) => `
      <tr>
        <td>${row.source}</td>
        <td>${row.model}</td>
        <td>${formatTokens(row.total_tokens)}</td>
      </tr>
    `).join("")
    : `<tr><td colspan="3" class="empty-cell">当前范围暂无模型数据</td></tr>`;
}

function showChartTooltip(point) {
  const container = $("#line-chart");
  const tooltip = container.querySelector(".chart-tooltip") || document.createElement("div");
  const box = container.getBoundingClientRect();
  const cx = Number(point.getAttribute("cx"));
  const cy = Number(point.getAttribute("cy"));
  const svg = point.ownerSVGElement;
  const viewBox = svg.viewBox.baseVal;
  const x = (cx / viewBox.width) * box.width;
  const y = (cy / viewBox.height) * box.height;

  tooltip.className = "chart-tooltip";
  tooltip.innerHTML = chartTooltip(point.dataset.tooltipDate || "", point.dataset.tooltipTokens || "0");
  tooltip.style.left = `${Math.min(box.width - 120, Math.max(12, x - 60))}px`;
  tooltip.style.top = `${Math.max(10, y - 58)}px`;
  if (!tooltip.parentElement) container.appendChild(tooltip);
}

function hideChartTooltip() {
  $("#line-chart").querySelector(".chart-tooltip")?.remove();
}

function renderSummaryTable() {
  const rows = visibleCalendarRows();
  if (!rows.length) {
    $("#summary-table").innerHTML = `
      <tr><td colspan="8" class="empty-cell">当前周期暂无可显示日期</td></tr>
    `;
    return;
  }
  $("#summary-table").innerHTML = rows.map((row) => {
    return `
      <tr>
        <td>${row.date}</td>
        <td>${formatTokens(row.total_tokens)}</td>
        <td>${formatCount(row.event_count)}</td>
        <td>${formatTokens(row.input_tokens)}</td>
        <td>${formatTokens(row.output_tokens)}</td>
        <td>${formatTokens(row.cache_read_tokens)}</td>
        <td>${formatTokens(row.cache_write_tokens)}</td>
        <td>${formatTokens(row.reasoning_tokens)}</td>
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
  renderLineChart();
  const detail = await fetchJson(`/api/dashboard/day-detail?date=${encodeURIComponent(date)}`);
  if (!detail) return;
  $("#day-title").textContent = `${detail.date} 日详情`;
  const b = detail.breakdown;
  $("#day-breakdown").innerHTML = `
    <div class="breakdown-grid">
      <div class="mini-stat"><span>Tokens (M)</span><strong>${formatTokens(detail.total_tokens)}</strong></div>
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

function setCopyTokenStatus(text, variant = "") {
  const button = $("#copy-token");
  button.textContent = text;
  button.dataset.status = variant;
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
    const button = $("#copy-token");
    const token = state.profile?.plain_token || "";
    if (!token) {
      setCopyTokenStatus("无 token", "error");
      setTimeout(() => setCopyTokenStatus("复制 token"), 2000);
      return;
    }
    button.disabled = true;
    try {
      await navigator.clipboard.writeText(token);
      setCopyTokenStatus("已复制", "success");
    } catch {
      setCopyTokenStatus("复制失败", "error");
    } finally {
      setTimeout(() => {
        button.disabled = false;
        setCopyTokenStatus("复制 token");
      }, 2000);
    }
  });
  $("#logout").addEventListener("click", async () => {
    await fetch("/api/logout", { method: "POST" });
    window.location.assign("/login");
  });
}

bindControls();
loadProfile().then(loadDashboard);
