import { api } from "../api.js";
import { zar, zarPrecise, num, pct, mult, niceDate, esc } from "../format.js";
import { loading, errorState } from "../states.js";
import { icons } from "../icons.js";

const COMPONENTS = [
  ["ticket_performance", "Ticket Performance"],
  ["revenue", "Revenue"],
  ["sell_through", "Sell-Through"],
  ["social_engagement", "Social Engagement"],
  ["audience_growth", "Audience Growth"],
  ["marketing_efficiency", "Marketing Efficiency"],
];

export async function renderArtists(root) {
  const id = artistIdFromHash();
  return id ? renderDetail(root, id) : renderList(root);
}

function artistIdFromHash() {
  const q = location.hash.split("?")[1] || "";
  return new URLSearchParams(q).get("id");
}

// =============================================================================
// LIST / leaderboard
// =============================================================================
async function renderList(root) {
  loading(root, "Ranking the artists…");
  let data, fcData;
  try {
    [data, fcData] = await Promise.all([api.artists(), api.forecasts()]);
  } catch (e) {
    return errorState(root, e);
  }

  const artists = data.artists;
  if (artists.length === 0) {
    root.innerHTML = `<div class="panel"><div class="state"><h3>No artists yet</h3><p>Add artists and events to see the leaderboard.</p></div></div>`;
    return;
  }

  // Highlights (artists arrive sorted by score, best first).
  const byScore = artists;
  const bestSocial = [...artists].sort((a, b) => b.avg_engagement_rate - a.avg_engagement_rate)[0];
  const bestCommercial = [...artists].sort((a, b) => b.revenue_cents - a.revenue_cents)[0];
  const needsAttention = byScore[byScore.length - 1];

  // Seasonality (when shows perform best) + artists without enough history to
  // forecast — both relocated here from the old Forecasts page.
  const season = fcData.seasonality || [];
  const maxSeason = Math.max(...season.map((s) => s.avg_sell_through), 0.01);
  const noHistory = (fcData.forecasts || []).filter((f) => f.expected_tickets <= 0);

  root.innerHTML = `
    <div class="section-label">Which artists perform best for Sankayi?</div>

    <section class="top-grid">
      ${topCard("lead", "Top Performer", byScore[0], `${pct(byScore[0].avg_sell_through)} sell-through`, byScore[0].performance_score)}
      ${topCard("", "Best Social", bestSocial, `${pct(bestSocial.avg_engagement_rate, 1)} engagement`, bestSocial.performance_score)}
      ${topCard("", "Best Commercial", bestCommercial, `${zar(bestCommercial.revenue_cents)} revenue`, bestCommercial.performance_score)}
      ${topCard("warn", "Needs Attention", needsAttention, `${pct(needsAttention.avg_sell_through)} sell-through`, needsAttention.performance_score)}
    </section>

    ${season.length ? `
      <div class="section-label">When shows perform best · by month</div>
      <div class="panel">
        <div class="panel-head"><div><h2>Seasonality</h2><div class="hint">Average sell-through of completed shows — time your strongest acts to the strongest months</div></div><span class="trust calculated">Calc</span></div>
        <div class="mini-bars" style="margin-top:6px;">
          ${season.map((s) => `<div class="mini-bar"><span class="mbl">${esc(s.month)} · ${s.shows} show${s.shows === 1 ? "" : "s"}</span><div class="mbt"><div class="mbf" style="width:${Math.min((s.avg_sell_through / maxSeason) * 100, 100).toFixed(1)}%"></div></div><span class="mbv">${pct(s.avg_sell_through)}</span></div>`).join("")}
        </div>
      </div>` : ""}

    <div class="section-label">Full leaderboard</div>
    <div class="panel">
      <div class="panel-head"><div><h2>Artist Performance</h2><div class="hint">Ranked by weighted score · click a row for the full profile</div></div><span class="trust calculated">Calc</span></div>
      <div class="tbl-wrap">
        <table class="camp">
          <thead><tr>
            <th>Artist</th><th>Events</th><th>Tickets</th><th>Sell-Through</th>
            <th>Revenue</th><th>Engagement</th><th>ROI</th><th>Score</th><th></th>
          </tr></thead>
          <tbody id="artist-rows">
            ${byScore.map((a, i) => `
              <tr class="clickable" data-id="${a.id}">
                <td><div class="c-name">${rank(i)} ${esc(a.name)}</div><div class="c-sub">${esc(a.genre || "—")}</div></td>
                <td>${num(a.events_count)}</td>
                <td>${num(a.tickets_sold)}</td>
                <td>${pct(a.avg_sell_through)}</td>
                <td>${zar(a.revenue_cents)}</td>
                <td>${pct(a.avg_engagement_rate, 1)}</td>
                <td>${mult(a.roi)}</td>
                <td><div class="roas-cell"><span class="rv">${a.performance_score.toFixed(1)}</span><span class="rb"><i style="width:${a.performance_score}%"></i></span></div></td>
                <td class="chev-cell">${icons.chevron}</td>
              </tr>`).join("")}
          </tbody>
        </table>
      </div>
    </div>

    ${noHistory.length ? `<p class="set-note">${noHistory.map((f) => esc(f.artist_name)).join(", ")} ${noHistory.length === 1 ? "is" : "are"} not yet forecastable — needs at least one completed Sankayi event.</p>` : ""}
  `;

  root.querySelector("#artist-rows").addEventListener("click", (e) => {
    const tr = e.target.closest("tr[data-id]");
    if (tr) location.hash = `#/artists?id=${tr.dataset.id}`;
  });
}

function topCard(variant, label, a, metric, score) {
  return `
    <div class="top-card ${variant}">
      <span class="tl">${label}</span>
      <div class="tn">${esc(a.name)}</div>
      <div class="tm">${metric}</div>
      <div class="ts"><b>${score.toFixed(1)}</b> <span>/100 score</span></div>
    </div>`;
}

function rank(i) { return `<span style="color:var(--gold);font-family:var(--font-display)">${i + 1}.</span>`; }

// =============================================================================
// DETAIL / profile
// =============================================================================
async function renderDetail(root, id) {
  loading(root, "Loading artist profile…");
  let a, fcData;
  try {
    [a, fcData] = await Promise.all([api.artist(id), api.forecasts()]);
  } catch (e) {
    return errorState(root, e);
  }

  const forecast = (fcData.forecasts || []).find((f) => f.artist_id === id);
  const bd = a.score_breakdown || {};
  const maxTickets = Math.max(...(a.events || []).map((e) => e.tickets_sold), 1);

  root.innerHTML = `
    <div class="back-link" id="back">${icons.chevron} All artists</div>

    <div class="artist-head">
      <div>
        <h1 class="name">${esc(a.name)}</h1>
        <div class="chips">
          ${a.genre ? `<span class="chip gold">${esc(a.genre)}</span>` : ""}
          ${a.instagram_handle ? `<span class="chip">@${esc(a.instagram_handle)}</span>` : ""}
          <span class="chip">${num(a.events_count)} event${a.events_count === 1 ? "" : "s"} at Sankayi</span>
        </div>
      </div>
      <span class="trust calculated">Calculated indicator</span>
    </div>

    <section class="panel">
      <div class="panel-head"><div><h2>Artist Performance Score</h2><div class="hint">Weighted 0–100 · normalised across all Sankayi artists</div></div></div>
      <div class="score-hero">
        <div class="score-big">
          <div class="n">${a.performance_score.toFixed(1)}</div>
          <div class="o">out of 100</div>
          <div class="lbl">Overall score</div>
        </div>
        <div class="breakdown">
          ${COMPONENTS.map(([key, label]) => key === "audience_growth" ? bdPending(label) : bdRow(label, bd[key] ?? 0)).join("")}
        </div>
      </div>
      <p style="color:var(--ink-soft); font-size:13px; margin:18px 0 0; border-top:1px solid var(--line); padding-top:14px;">
        ${narrative(a, bd)}
      </p>
    </section>

    <div class="section-label">The numbers</div>
    <section class="stat-grid">
      ${stat(num(a.tickets_sold), "Tickets sold")}
      ${stat(money(a.revenue_cents), "Total revenue")}
      ${stat(pct(a.avg_sell_through), "Avg sell-through")}
      ${stat(money(a.avg_ticket_price_cents), "Avg ticket price")}
      ${stat(pct(a.avg_engagement_rate, 1), "Social engagement")}
      ${stat(money(a.spend_cents), "Marketing spend")}
      ${stat(money(a.attributed_revenue_cents), "Attributed revenue")}
      ${stat(mult(a.roi), "Marketing ROI")}
    </section>

    ${forecastPanel(forecast, a)}

    <div class="section-label">Event history</div>
    <div class="panel">
      <div class="panel-head"><div><h2>Ticket Performance by Event</h2><div class="hint">Every Sankayi night for ${esc(a.name)}</div></div><span class="trust actual">Actual</span></div>
      <div class="ev-list">
        ${(a.events || []).length === 0 ? `<div class="state"><p>No events recorded yet.</p></div>` :
          a.events.map((e) => evRow(e, maxTickets)).join("")}
      </div>
    </div>
  `;

  root.querySelector("#back").addEventListener("click", () => { location.hash = "#/artists"; });
}

function bdRow(label, val) {
  return `
    <div class="bd-row">
      <span class="k">${label}</span>
      <div class="bd-track"><div class="bd-fill" style="width:${Math.max(Math.min(val, 100), 0)}%"></div></div>
      <span class="v">${Math.round(val)}</span>
    </div>`;
}

// audience_growth is a Phase-4 placeholder (0 for everyone); showing 0/100 would
// unfairly read as poor performance, so we mark it honestly as not-yet-tracked.
function bdPending(label) {
  return `
    <div class="bd-row">
      <span class="k">${label}</span>
      <span style="font-size:11.5px; color:var(--ink-mute); font-style:italic;">Not yet tracked · Phase 4</span>
      <span class="v" style="color:var(--ink-mute);">—</span>
    </div>`;
}

function stat(value, label) {
  return `<div class="stat"><div class="n">${value}</div><div class="l">${label}</div></div>`;
}

function evRow(e, maxTickets) {
  const cls = e.sell_through >= 0.8 ? "hi" : e.sell_through >= 0.6 ? "mid" : "low";
  const status = e.status === "completed"
    ? `<span class="pill done"><span class="d"></span> Completed</span>`
    : `<span class="pill active"><span class="d"></span> ${esc(cap(e.status.replace("_", " ")))}</span>`;
  return `
    <div class="ev-row">
      <div><div class="ev-name">${esc(e.title.replace("Sankayi Jazz Series: ", ""))}</div><div class="ev-sub">${niceDate(e.event_date)}</div></div>
      <div class="st-track"><div class="st-fill ${cls}" style="width:${(e.sell_through * 100).toFixed(1)}%"></div></div>
      <div style="text-align:right; font-variant-numeric:tabular-nums; color:var(--ink); font-size:13.5px;">${num(e.tickets_sold)} · ${pct(e.sell_through)}</div>
      <div class="ev-rev">${money(e.revenue_cents)} ${status}</div>
    </div>`;
}

function narrative(a, bd) {
  const strengths = COMPONENTS.filter(([k]) => k !== "audience_growth")
    .map(([k, l]) => [l, bd[k] ?? 0])
    .sort((x, y) => y[1] - x[1]);
  const top = strengths[0], low = strengths[strengths.length - 1];
  return `${esc(a.name)} scores highest on <b style="color:var(--cream)">${top[0]}</b> (${Math.round(top[1])}/100)${low[1] < 50 ? `, with room to grow on <b style="color:var(--cream)">${low[0]}</b> (${Math.round(low[1])}/100)` : ""}. This is an analytical indicator, not a verdict — it normalises performance against Sankayi's other artists.`;
}

// "If they return" forecast, relocated from the old Forecasts page. Renders only
// when the artist has enough completed history to project (expected_tickets > 0).
function forecastPanel(f, a) {
  if (!f || f.expected_tickets <= 0) return "";
  const span = Math.max(f.range_high - f.range_low, 1);
  const pos = Math.min(Math.max((f.expected_tickets - f.range_low) / span, 0), 1) * 100;
  return `
    <div class="section-label">If they return · forecast</div>
    <div class="panel">
      <div class="panel-head"><div><h2>Artist Performance Forecast</h2><div class="hint">Projected from ${esc(a.name)}'s completed Sankayi show(s) — a forecast, not a guarantee</div></div><span class="trust forecast">Forecast</span></div>
      <div class="fc-expected" style="margin-top:6px;"><span class="n">${num(f.expected_tickets)}</span><span class="u">expected tickets · ${pct(f.expected_sell_through)} sell-through</span></div>
      <div class="fc-range" style="margin-top:12px;">
        <div class="track"><div class="band" style="left:0;right:0;"></div><div class="mark" style="left:${pos}%"></div></div>
        <div class="lab"><span>Low ${num(f.range_low)}</span><span>Expected ${num(f.expected_tickets)}</span><span>High ${num(f.range_high)}</span></div>
      </div>
      <div class="fc-mini" style="margin-top:16px;">
        <div class="m"><div class="n">${money(f.expected_revenue_cents)}</div><div class="l">Est. revenue</div></div>
        <div class="m"><div class="n">${money(f.recommended_budget_cents)}</div><div class="l">Rec. ad budget</div></div>
        <div class="m"><div class="n">${pct(f.confidence)}</div><div class="l">Confidence</div></div>
      </div>
    </div>`;
}

function money(cents) { return `<span class="cur">R</span>${num(Math.round(cents / 100))}`; }
function cap(s) { return String(s || "").charAt(0).toUpperCase() + String(s || "").slice(1); }
