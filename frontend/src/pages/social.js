import { api } from "../api.js";
import { num, pct, delta, niceDate, shortDate, esc } from "../format.js";
import { areaChartHTML, drawAreaChart } from "../charts.js";
import { loading, errorState } from "../states.js";

export async function renderSocial(root) {
  loading(root, "Loading audience & social…");
  let s;
  try { s = await api.social(); } catch (e) { return errorState(root, e); }

  const series = s.account_series || [];
  const latest = s.latest || {};

  // Followers are only available for the last ~30 days (Graph API caps
  // follower_count there); earlier days are reach-only with null followers.
  // Anchor the follower line & growth delta on the days that actually have one.
  const followerDays = series.filter((p) => p.followers != null);
  const first = followerDays[0] || {};
  const last = followerDays[followerDays.length - 1] || {};

  const followerPoints = followerDays.map((p) => ({ label: shortDate(p.date), value: p.followers, tip: niceDate(p.date) }));

  // Reach chart is grouped by month (the full series overflows a single row).
  const months = [...new Set(series.map((p) => p.date.slice(0, 7)))].sort();
  let reachMonth = months[months.length - 1] || "all";

  // Top posts are paginated (147 posts is a lot to scroll).
  const PER_PAGE = 10;
  const totalPages = Math.max(1, Math.ceil((s.top_posts?.length || 0) / PER_PAGE));
  let postPage = 0;

  root.innerHTML = `
    <div class="section-label">Is Sankayi's social actually growing and driving interest?</div>

    <section class="kpi-grid cols-6">
      ${kpi("Followers", num(latest.followers), "actual", `<span class="delta up">▲ ${delta(s.followers_growth)}</span>`, true)}
      ${kpi("Follower Growth", delta(s.followers_growth), "calculated", `${num(first.followers)} → ${num(last.followers)}`)}
      ${kpi("Reach", num(latest.reach), "actual", "latest day")}
      ${kpi("Engagement Rate", pct(latest.engagement_rate, 1), "calculated", "engagements ÷ reach")}
      ${kpi("Profile Visits", num(latest.profile_visits), "actual", "latest day")}
      ${kpi("Avg Post Engagement", pct(s.avg_post_engagement, 1), "calculated", `${s.top_posts.length} posts`)}
    </section>

    <section class="row hero">
      <div class="panel">
        <div class="panel-head">
          <div><h2>Audience Growth</h2><div class="hint">Follower count over time</div></div>
          <span class="legend-chip"><span class="sw"></span> Followers</span>
        </div>
        ${areaChartHTML("followers")}
      </div>
      <div class="panel insight">
        <span class="spark">Growth Signal</span>
        <h3>${growthHeadline(s.followers_growth)}</h3>
        <p>Sankayi added <b>${num((last.followers || 0) - (first.followers || 0))} followers</b> over the tracked window — from ${num(first.followers)} to ${num(last.followers)}. Engagement is running at <b>${pct(latest.engagement_rate, 1)}</b> of reach.</p>
        <div class="spark-viz"><div class="viz-cap">Follower trend · last ${followerDays.length} days</div>${sparkline(followerDays.map((p) => p.followers))}</div>
        <div class="metric-row">
          <div class="metric"><div class="n">${delta(s.followers_growth)}</div><div class="l">Growth</div></div>
          <div class="metric"><div class="n">${num(latest.reach)}</div><div class="l">Reach</div></div>
          <div class="metric"><div class="n">${num(latest.profile_visits)}</div><div class="l">Profile visits</div></div>
        </div>
      </div>
    </section>

    <div class="section-label">Reach per day</div>
    <div class="panel">
      <div class="panel-head">
        <div><h2>Reach & Engagement</h2><div class="hint">Accounts reached each day, by month</div></div>
        <select id="reach-month" class="mini-select">
          ${months.map((m) => `<option value="${m}"${m === reachMonth ? " selected" : ""}>${monthLabel(m)}</option>`).join("")}
          <option value="all"${reachMonth === "all" ? " selected" : ""}>All months</option>
        </select>
      </div>
      <div class="vbars" id="reach-bars">${reachBarsFor(series, reachMonth)}</div>
    </div>

    <div class="section-label">Top performing posts</div>
    <div class="panel">
      <div class="panel-head"><div><h2>Top Performing Posts</h2><div class="hint">Ranked by engagement rate</div></div><span class="trust calculated">Calc</span></div>
      <div class="tbl-wrap">
        <table class="camp">
          <thead><tr>
            <th>Post</th><th>Reach</th><th>Likes</th><th>Comments</th><th>Shares</th><th>Saves</th><th>Engagement</th>
          </tr></thead>
          <tbody id="top-posts-body">
            ${(s.top_posts || []).slice(0, PER_PAGE).map(postRow).join("")}
          </tbody>
        </table>
      </div>
      <div class="pager">
        <button id="pp-prev" class="pg-btn" disabled>← Prev</button>
        <span id="pp-ind" class="pg-ind">Page 1 of ${totalPages}</span>
        <button id="pp-next" class="pg-btn"${totalPages <= 1 ? " disabled" : ""}>Next →</button>
      </div>
    </div>
  `;

  drawAreaChart(root, "followers", followerPoints, { unit: "followers" });

  // Reach month selector — re-render the bars for the chosen month.
  const monthSel = root.querySelector("#reach-month");
  if (monthSel) {
    monthSel.addEventListener("change", () => {
      root.querySelector("#reach-bars").innerHTML = reachBarsFor(series, monthSel.value);
    });
  }

  // Top-posts pagination.
  const body = root.querySelector("#top-posts-body");
  const prev = root.querySelector("#pp-prev");
  const next = root.querySelector("#pp-next");
  const ind = root.querySelector("#pp-ind");
  const posts = s.top_posts || [];
  function renderPage() {
    const start = postPage * PER_PAGE;
    body.innerHTML = posts.slice(start, start + PER_PAGE).map(postRow).join("");
    ind.textContent = `Page ${postPage + 1} of ${totalPages}`;
    prev.disabled = postPage === 0;
    next.disabled = postPage >= totalPages - 1;
  }
  if (prev) prev.addEventListener("click", () => { if (postPage > 0) { postPage--; renderPage(); } });
  if (next) next.addEventListener("click", () => { if (postPage < totalPages - 1) { postPage++; renderPage(); } });
}

/** Month key "2026-05" → "May 2026". */
function monthLabel(ym) {
  const [y, m] = ym.split("-");
  return new Date(Number(y), Number(m) - 1, 1).toLocaleString("en", { month: "long", year: "numeric" });
}

/** Reach bars for one month (or "all"), engagements shown only where present. */
function reachBarsFor(series, month) {
  const pts = month === "all" ? series : series.filter((p) => p.date.slice(0, 7) === month);
  const mx = Math.max(...pts.map((p) => p.reach || 0), 1);
  return pts
    .map(
      (p) => `
        <div class="vbar">
          <div class="amt">${num(p.reach)}</div>
          <div class="track"><div class="fill" style="height:${Math.max(((p.reach || 0) / mx) * 100, 3)}%"></div></div>
          <div class="cap">${shortDate(p.date)}${p.engagements != null ? `<small>${num(p.engagements)} eng.</small>` : ""}</div>
        </div>`
    )
    .join("");
}

function kpi(label, value, trust, foot, footIsHtml) {
  return `
    <div class="kpi">
      <div class="top"><span class="label">${label}</span><span class="trust ${trust}">${trust === "calculated" ? "Calc" : "Actual"}</span></div>
      <div class="value">${value}</div>
      <div class="foot">${footIsHtml ? foot : `<span class="ctx">${foot}</span>`}</div>
    </div>`;
}

function postRow(p) {
  const type = (p.post_type || "image").toLowerCase();
  const cap = p.caption ? esc(p.caption) : esc(p.event_title || "Untitled post");
  return `
    <tr>
      <td>
        <div class="post-meta">
          <span class="ptype ${type}">${esc(type)}</span>
          <span class="post-cap">${cap}</span>
        </div>
        <div class="post-meta"><span class="pm">${niceDate(p.published_at)}</span></div>
      </td>
      <td>${num(p.reach)}</td>
      <td>${num(p.likes)}</td>
      <td>${num(p.comments)}</td>
      <td>${num(p.shares)}</td>
      <td>${num(p.saves)}</td>
      <td><div class="roas-cell"><span class="rv">${pct(p.engagement_rate, 1)}</span></div></td>
    </tr>`;
}

/** Minimal inline-SVG sparkline (area + line) for a series of numbers. */
export function sparkline(vals, w = 520, h = 96) {
  const v = vals.filter((x) => x != null);
  if (v.length < 2) return "";
  const min = Math.min(...v), max = Math.max(...v), span = max - min || 1;
  const x = (i) => (i / (v.length - 1)) * w;
  const y = (val) => h - 6 - ((val - min) / span) * (h - 14);
  const line = v.map((val, i) => `${x(i).toFixed(1)},${y(val).toFixed(1)}`).join(" ");
  const id = "spk" + Math.round(min + max + v.length);
  return `<svg class="spark-svg" viewBox="0 0 ${w} ${h}" preserveAspectRatio="none">
    <defs><linearGradient id="${id}" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="var(--gold)" stop-opacity="0.35"/>
      <stop offset="1" stop-color="var(--gold)" stop-opacity="0"/>
    </linearGradient></defs>
    <polygon points="0,${h} ${line} ${w},${h}" fill="url(#${id})"/>
    <polyline points="${line}" fill="none" stroke="var(--gold)" stroke-width="2" vector-effect="non-scaling-stroke"/>
  </svg>`;
}

function growthHeadline(g) {
  if (g >= 0.1) return "Strong, steady audience growth.";
  if (g > 0) return "The audience is growing.";
  if (g === 0) return "Audience is holding steady.";
  return "Audience dipped this period.";
}
