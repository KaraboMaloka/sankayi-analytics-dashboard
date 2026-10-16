import { api } from "../api.js";
import { zar, num, pct, mult, delta, niceDate, shortDate, esc } from "../format.js";
import { areaChartHTML, drawAreaChart } from "../charts.js";
import { loading, errorState } from "../states.js";
import { filters, inPeriod, matchesSearch } from "../filters.js";

export async function renderOverview(root) {
  loading(root, "Loading the command centre…");

  let dash, events, social, artists, health, salesResp, forecasts;
  try {
    [dash, events, social, artists, health, salesResp, forecasts] = await Promise.all([
      api.dashboard(), api.events(), api.social(), api.artists(),
      api.health(), api.sales({ pageSize: 20000 }), api.forecasts(),
    ]);
  } catch (e) {
    return errorState(root, e);
  }

  // ---- apply global topbar filters (period / event / search) ----
  const f = filters.get();
  const nameById = new Map(events.events.map((e) => [e.id, e.artist?.name || e.title]));
  const evFiltered = events.events.filter(
    (e) => inPeriod(e.event_date, f.period) && (!f.eventId || e.id === f.eventId) && matchesSearch(e.artist?.name || e.title, f.search)
  );
  const salesFiltered = salesResp.sales.filter(
    (s) => (!f.eventId || s.event_id === f.eventId) && inPeriod(s.sold_at, f.period) && matchesSearch(nameById.get(s.event_id), f.search)
  );

  // KPIs recomputed from the filtered set so the controls actually move them.
  const ticketsSold = salesFiltered.reduce((a, s) => a + s.quantity, 0);
  const revenueCents = salesFiltered.reduce((a, s) => a + s.quantity * s.price_cents, 0);
  // Average price is over PAID tickets only — free comps would drag it below the
  // real ticket price otherwise.
  const paidTickets = salesFiltered.reduce((a, s) => a + (s.price_cents > 0 ? s.quantity : 0), 0);
  const capTotal = evFiltered.reduce((a, e) => a + e.capacity, 0);
  const sellThrough = capTotal ? ticketsSold / capTotal : 0;

  // ---- cumulative ticket-sales series (from the filtered sales feed) ----
  const byDate = new Map();
  for (const s of salesFiltered) {
    const d = s.sold_at.slice(0, 10);
    byDate.set(d, (byDate.get(d) || 0) + s.quantity);
  }
  const dates = [...byDate.keys()].sort();
  let cum = 0;
  const series = dates.map((d) => {
    cum += byDate.get(d);
    return { label: shortDate(d), value: cum, tip: niceDate(d) };
  });

  // ---- dynamic insight (top artist) ----
  const top = artists.artists[0];
  const topActs = (artists.artists || []).slice(0, 3);
  const maxActTix = Math.max(...topActs.map((a) => a.tickets_sold), 1);
  const insight = top
    ? `<b>${esc(top.name)}</b> is Sankayi's strongest draw — ${pct(top.avg_sell_through)} average sell-through and a ${mult(top.roi)} return on marketing. ${esc(top.genre || "")} billing is over-indexing with the audience.`
    : "Not enough artist history yet to surface a headline insight.";

  // ---- upcoming event: the SOONEST future show (not just the first on_sale in
  // list order — that was surfacing the furthest-out event). ----
  const today = new Date();
  const todayStr = today.toISOString().slice(0, 10);
  const future = evFiltered
    .filter((e) => e.event_date >= todayStr)
    .sort((a, b) => a.event_date.localeCompare(b.event_date));
  const upcoming = future[0] || [...evFiltered].sort((a, b) => b.event_date.localeCompare(a.event_date))[0];

  // ---- revenue + sell-through (top 10 each — full lists overflow the panels) ----
  const topRevenue = [...evFiltered].sort((a, b) => b.revenue_cents - a.revenue_cents).slice(0, 10);
  const topSellThrough = [...evFiltered].sort((a, b) => b.sell_through - a.sell_through).slice(0, 10);
  const maxRev = Math.max(...topRevenue.map((e) => e.revenue_cents), 1);

  root.innerHTML = `
    <section class="kpi-grid cols-6">
      ${kpi("Tickets Sold", num(ticketsSold), "actual", `across ${evFiltered.length} event${evFiltered.length === 1 ? "" : "s"}`)}
      ${kpi("Ticket Revenue", money(revenueCents), "actual", `${zar(Math.round(revenueCents / Math.max(paidTickets, 1)))} avg / paid ticket`)}
      ${kpi("Avg Sell-Through", pct(sellThrough), "calculated", `${num(ticketsSold)} seats sold`)}
      ${kpi("IG Followers", num(dash.followers), "actual", `<span class="delta up">▲ ${delta(dash.followers_growth)}</span>`, true)}
      ${kpi("Marketing Spend", money(dash.ad_spend_cents), "actual", "on boosted posts")}
      ${kpi("Blended ROAS", mult(dash.roas), "calculated", `on ${zar(dash.ad_spend_cents)} spend`)}
    </section>

    <section class="row hero">
      <div class="panel">
        <div class="panel-head">
          <div><h2>Ticket Sales Over Time</h2><div class="hint">Cumulative tickets sold across the series</div></div>
          <span class="legend-chip"><span class="sw"></span> Cumulative tickets</span>
        </div>
        ${areaChartHTML("sales")}
      </div>
      <div class="panel insight">
        <span class="spark">Sales Insight</span>
        <h3>${top ? esc(top.name) + " is leading the series." : "Series overview"}</h3>
        <p>${insight}</p>
        ${topActs.length ? `<div class="viz-cap">Top acts by tickets sold</div>${miniBars(topActs.map((a) => ({ label: esc(a.name), pct: a.tickets_sold / maxActTix, value: num(a.tickets_sold) })))}` : ""}
        <div class="metric-row">
          <div class="metric"><div class="n">${top ? pct(top.avg_sell_through) : "—"}</div><div class="l">Sell-through</div></div>
          <div class="metric"><div class="n">${top ? mult(top.roi) : "—"}</div><div class="l">Marketing ROI</div></div>
          <div class="metric"><div class="n">${num(top?.tickets_sold || 0)}</div><div class="l">Tickets</div></div>
        </div>
      </div>
    </section>

    <section class="row split">
      <div class="panel">
        <div class="panel-head"><div><h2>Revenue by Event</h2><div class="hint">Top 10 by gross ticket revenue · ZAR</div></div><span class="trust actual">Actual</span></div>
        <div class="st-list">
          ${topRevenue.map((e) => revRow(esc(e.artist?.name || e.title), niceDate(e.event_date), e.revenue_cents, maxRev)).join("")}
        </div>
      </div>

      <div class="panel">
        <div class="panel-head"><div><h2>Sell-Through</h2><div class="hint">Top 10 by sell-through · sold ÷ capacity</div></div><span class="trust calculated">Calc</span></div>
        <div class="st-list">
          ${topSellThrough.map((e) => stRow(esc(e.artist?.name || e.title), e.sell_through)).join("")}
          <div class="st-row" style="margin-top:6px; border-top:1px solid var(--line); padding-top:15px;">
            <span class="name" style="color:var(--gold); font-weight:600;">Series average</span>
            <div class="st-track"><div class="st-fill mid" style="width:${(sellThrough * 100).toFixed(1)}%; opacity:.7;"></div></div>
            <span class="pct" style="color:var(--gold);">${pct(sellThrough)}</span>
          </div>
        </div>
      </div>
    </section>

    <div class="section-label">Where attention is needed</div>

    <section class="row bottom">
      ${upcomingCard(upcoming, forecasts, today)}
      <div class="panel">
        <div class="panel-head"><div><h2>Artist Leaderboard</h2><div class="hint">Weighted performance score</div></div><span class="trust calculated">Calc</span></div>
        <div class="lb">
          ${artists.artists.slice(0, 5).map((a, i) => `
            <div class="lb-row">
              <div class="lb-rank">${i + 1}</div>
              <div>
                <div class="lb-name">${esc(a.name)}</div>
                <div class="lb-sub">${num(a.tickets_sold)} tickets · ${money(a.revenue_cents)} · ${pct(a.avg_sell_through)} sell-through</div>
                <div class="lb-bar"><div class="f" style="width:${a.performance_score}%"></div></div>
              </div>
              <div class="lb-score"><span class="s">${a.performance_score.toFixed(1)}</span><span class="o">/100</span></div>
            </div>`).join("")}
        </div>
      </div>
    </section>

    <div class="section-label">Can I trust today's numbers?</div>
    <section class="health">
      ${health.sources.map(healthCard).join("")}
      <div class="hcard"><div class="h-top"><span class="src">Database</span><span class="status ${health.database === "healthy" ? "ok" : "err"}"><span class="led"></span> ${esc(cap(health.database))}</span></div><div class="h-sub">PostgreSQL · snapshots current</div></div>
    </section>
  `;

  drawAreaChart(root, "sales", series, { unit: "tickets" });
}

// ---- small builders ----
function money(cents) { return `<span class="cur">R</span>${num(Math.round(cents / 100))}`; }

/** Horizontal mini-bars: items = [{label, pct (0..1), value}]. */
function miniBars(items) {
  return `<div class="mini-bars">${items
    .map(
      (it) => `<div class="mini-bar"><span class="mbl">${it.label}</span>
        <div class="mbt"><div class="mbf" style="width:${Math.max(it.pct * 100, 3).toFixed(1)}%"></div></div>
        <span class="mbv">${it.value}</span></div>`
    )
    .join("")}</div>`;
}

function kpi(label, value, trust, foot, footIsHtml) {
  const trustLabel = trust === "calculated" ? "Calc" : "Actual";
  return `
    <div class="kpi">
      <div class="top"><span class="label">${label}</span><span class="trust ${trust}">${trustLabel}</span></div>
      <div class="value">${value}</div>
      <div class="foot">${footIsHtml ? foot : `<span class="ctx">${foot}</span>`}</div>
    </div>`;
}

function revRow(name, date, cents, maxRev) {
  const w = Math.max((cents / maxRev) * 100, 2).toFixed(1);
  return `
    <div class="st-row">
      <span class="name">${name}<small style="display:block;color:var(--ink-soft);font-size:11px;">${date}</small></span>
      <div class="st-track"><div class="st-fill hi" style="width:${w}%"></div></div>
      <span class="pct">${money(cents)}</span>
    </div>`;
}

function stRow(name, ratio) {
  const cls = ratio >= 0.8 ? "hi" : ratio >= 0.6 ? "mid" : "low";
  return `
    <div class="st-row">
      <span class="name">${name}</span>
      <div class="st-track"><div class="st-fill ${cls}" style="width:${(ratio * 100).toFixed(1)}%"></div></div>
      <span class="pct">${pct(ratio)}</span>
    </div>`;
}

function upcomingCard(e, forecasts, today) {
  if (!e) return `<div class="panel"><div class="state"><h3>No upcoming events</h3><p>Announce the next Jazz Series night to see it here.</p></div></div>`;
  const remaining = Math.max(e.capacity - e.tickets_sold, 0);
  const days = Math.round((new Date(e.event_date) - today) / 86400000);
  const fc = forecasts.forecasts?.find((f) => f.artist_id === e.artist?.id);
  const hasForecast = fc && fc.expected_tickets > e.tickets_sold;
  const note = hasForecast
    ? `<span class="trust forecast">Forecast</span><span>Tracking toward <b>~${num(fc.expected_tickets)} tickets (${pct(fc.expected_sell_through)})</b> · range ${num(fc.range_low)}–${num(fc.range_high)} · confidence ${pct(fc.confidence)}.</span>`
    : `<span class="trust forecast">Forecast</span><span>Not enough completed history for this artist yet — the forecast sharpens after their first Sankayi event.</span>`;
  return `
    <div class="panel upcoming">
      <span class="up-tag">Next Event${days > 0 ? ` · in ${days} day${days === 1 ? "" : "s"}` : days === 0 ? " · today" : ""}</span>
      <div class="up-title">${esc(e.artist?.name || e.title)}</div>
      <div class="up-meta">
        ${e.artist?.genre ? `<span>🎷 <b>${esc(e.artist.genre)}</b></span>` : ""}
        <span>📅 <b>${niceDate(e.event_date)}</b></span>
        <span>📍 <b>Sankayi Restaurant &amp; Lounge</b></span>
      </div>
      <div class="up-progress">
        <div class="bar"><div class="fill" style="width:${(e.sell_through * 100).toFixed(1)}%"></div></div>
        <div class="lab"><span>${num(e.tickets_sold)} of ${num(e.capacity)} sold</span><span>${num(remaining)} remaining</span></div>
      </div>
      <div class="up-stats">
        <div class="up-stat"><div class="n">${num(e.tickets_sold)}</div><div class="l">Tickets sold</div></div>
        <div class="up-stat"><div class="n">${pct(e.sell_through)}</div><div class="l">Sold to date</div></div>
        <div class="up-stat"><div class="n">${money(e.revenue_cents)}</div><div class="l">Revenue so far</div></div>
      </div>
      <div class="forecast-note">${note}</div>
    </div>`;
}

function healthCard(s) {
  const state = s.status === "success" ? "ok" : s.status === "partial" ? "warn" : "err";
  const label = s.status === "success" ? "Connected" : cap(s.status);
  const synced = s.last_synced ? niceDate(s.last_synced) + ", " + new Date(s.last_synced).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" }) : "never";
  return `
    <div class="hcard">
      <div class="h-top"><span class="src">${esc(cap(s.source.replace("_", " ")))}</span><span class="status ${state}"><span class="led"></span> ${esc(label)}</span></div>
      <div class="h-sub">Last sync ${esc(synced)} · ${s.message ? esc(s.message) : num(s.rows) + " records"}</div>
    </div>`;
}

function cap(s) { return String(s || "").charAt(0).toUpperCase() + String(s || "").slice(1); }
