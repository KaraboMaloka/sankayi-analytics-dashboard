import { api } from "../api.js";
import { zar, num, pct, mult, niceDate, shortDate, esc } from "../format.js";
import { areaChartHTML, drawAreaChart } from "../charts.js";
import { loading, errorState } from "../states.js";
import { icons } from "../icons.js";
import { filters, inPeriod, matchesSearch } from "../filters.js";

export async function renderEvents(root) {
  const id = idFromHash();
  return id ? renderDetail(root, id) : renderList(root);
}

function idFromHash() {
  return new URLSearchParams(location.hash.split("?")[1] || "").get("id");
}

export function eventPill(status) {
  const map = {
    on_sale: ["active", "On sale"],
    sold_out: ["hot", "Sold out"],
    completed: ["done", "Completed"],
    announced: ["warn", "Announced"],
    draft: ["done", "Draft"],
    cancelled: ["err", "Cancelled"],
  };
  const [cls, label] = map[status] || ["done", status];
  return `<span class="pill ${cls}"><span class="d"></span> ${esc(label)}</span>`;
}

// =============================================================================
// LIST
// =============================================================================
async function renderList(root) {
  loading(root, "Loading events…");
  let data;
  try { data = await api.events(); } catch (e) { return errorState(root, e); }

  const f = filters.get();
  const events = data.events.filter(
    (e) =>
      inPeriod(e.event_date, f.period) &&
      (!f.eventId || e.id === f.eventId) &&
      matchesSearch(e.artist?.name || e.title, f.search)
  );
  if (events.length === 0) {
    const msg = data.events.length === 0
      ? "Create a Jazz Series event to see it here."
      : "No events match the current filters — try widening the period or clearing the search.";
    root.innerHTML = `<div class="panel"><div class="state"><h3>No events${data.events.length ? " match" : " yet"}</h3><p>${msg}</p></div></div>`;
    return;
  }

  const tickets = events.reduce((a, e) => a + e.tickets_sold, 0);
  const revenue = events.reduce((a, e) => a + e.revenue_cents, 0);
  const cap = events.reduce((a, e) => a + e.capacity, 0);
  const avgSell = cap ? tickets / cap : 0;

  root.innerHTML = `
    <div class="section-label">Every Sankayi Jazz Series night</div>
    <section class="kpi-grid cols-4">
      ${kpi("Events", num(events.length), "actual", "in the series")}
      ${kpi("Tickets Sold", num(tickets), "actual", "all events")}
      ${kpi("Total Revenue", money(revenue), "actual", "gross · ZAR")}
      ${kpi("Avg Sell-Through", pct(avgSell), "calculated", `${num(tickets)} of ${num(cap)} seats`)}
    </section>

    <div class="section-label">All events</div>
    <section class="event-grid" id="event-grid">
      ${events.map(eventCard).join("")}
    </section>
  `;

  root.querySelector("#event-grid").addEventListener("click", (e) => {
    const card = e.target.closest("[data-id]");
    if (card) location.hash = `#/events?id=${card.dataset.id}`;
  });
}

function eventCard(e) {
  const cls = e.sell_through >= 0.8 ? "hi" : e.sell_through >= 0.6 ? "mid" : "low";
  return `
    <div class="event-card" data-id="${e.id}">
      <div class="ec-top">
        <div>
          <div class="ec-title">${esc(e.artist?.name || e.title)}</div>
          <div class="ec-sub">${niceDate(e.event_date)} · ${esc(e.artist?.genre || "Jazz")}</div>
        </div>
        ${eventPill(e.status)}
      </div>
      <div class="st-track"><div class="st-fill ${cls}" style="width:${(e.sell_through * 100).toFixed(1)}%"></div></div>
      <div class="ec-stats">
        <div class="ec-stat"><div class="n">${num(e.tickets_sold)}<span class="cur"> / ${num(e.capacity)}</span></div><div class="l">Tickets</div></div>
        <div class="ec-stat"><div class="n">${pct(e.sell_through)}</div><div class="l">Sell-through</div></div>
        <div class="ec-stat"><div class="n">${money(e.revenue_cents)}</div><div class="l">Revenue</div></div>
      </div>
    </div>`;
}

// =============================================================================
// DETAIL
// =============================================================================
async function renderDetail(root, id) {
  loading(root, "Loading event…");
  let e;
  try { e = await api.event(id); } catch (err) { return errorState(root, err); }

  const remaining = Math.max(e.capacity - e.tickets_sold, 0);
  const velocity = (e.sales_velocity || []).map((v) => ({
    label: shortDate(v.date), value: v.cumulative_tickets, tip: niceDate(v.date),
  }));
  const maxType = Math.max(...(e.sales_by_type || []).map((t) => t.quantity), 1);

  // Sales channel (how this show's tickets were bought). `quicket` = Online.
  const channel = { online: 0, door: 0, complimentary: 0, manual: 0 };
  (e.sales_by_channel || []).forEach((c) => {
    const k = c.channel === "quicket" ? "online" : (channel[c.channel] !== undefined ? c.channel : "manual");
    channel[k] += c.quantity;
  });
  const chTotal = channel.online + channel.door + channel.complimentary + channel.manual || 1;

  root.innerHTML = `
    <div class="back-link" id="back">${icons.chevron} All events</div>

    <div class="artist-head">
      <div>
        <h1 class="name">${esc(e.artist?.name || e.title)}</h1>
        <div class="chips">
          ${e.artist?.genre ? `<span class="chip gold">${esc(e.artist.genre)}</span>` : ""}
          <span class="chip">📅 ${niceDate(e.event_date)}</span>
          <span class="chip">📍 Sankayi Restaurant &amp; Lounge</span>
        </div>
      </div>
      ${eventPill(e.status)}
    </div>

    <section class="kpi-grid cols-4">
      ${kpi("Tickets Sold", `${num(e.tickets_sold)}`, "actual", `of ${num(e.capacity)} capacity`)}
      ${kpi("Revenue", money(e.revenue_cents), "actual", `at ${zar(e.ticket_price_cents)} face`)}
      ${kpi("Sell-Through", pct(e.sell_through), "calculated", `${num(remaining)} seats left`)}
      ${kpi("Ticket Types", num((e.sales_by_type || []).length), "actual", "on sale")}
    </section>

    <section class="row hero">
      <div class="panel">
        <div class="panel-head"><div><h2>Sales Velocity</h2><div class="hint">Cumulative tickets sold toward the event</div></div><span class="legend-chip"><span class="sw"></span> Cumulative</span></div>
        ${velocity.length ? areaChartHTML("velocity") : `<div class="state"><p>No sales recorded yet.</p></div>`}
      </div>
      <div class="panel">
        <div class="panel-head"><div><h2>Sales by Type</h2><div class="hint">Tickets & revenue per tier</div></div><span class="trust actual">Actual</span></div>
        <div class="st-list">
          ${(e.sales_by_type || []).map((t) => `
            <div class="st-row">
              <span class="name">${esc(t.ticket_type)}</span>
              <div class="st-track"><div class="st-fill mid" style="width:${(t.quantity / maxType * 100).toFixed(1)}%"></div></div>
              <span class="pct" style="font-size:13px;">${num(t.quantity)}</span>
            </div>
            <div style="display:flex; justify-content:flex-end; font-size:11.5px; color:var(--ink-mute); margin:-6px 0 2px;">${money(t.revenue_cents)}</div>
          `).join("") || `<div class="state"><p>No sales yet.</p></div>`}
        </div>
      </div>
    </section>

    <div class="panel">
      <div class="panel-head"><div><h2>Sales Channel</h2><div class="hint">How this show's tickets were bought — online, door &amp; complimentary</div></div><span class="trust actual">Actual</span></div>
      <div style="display:flex; height:16px; border-radius:8px; overflow:hidden; margin-top:14px; border:1px solid var(--line);">
        <div style="width:${channel.online / chTotal * 100}%; background:linear-gradient(90deg,var(--gold-dim),var(--gold-hi));"></div>
        <div style="width:${channel.door / chTotal * 100}%; background:linear-gradient(90deg,var(--indigo-hi),#5a52b0);"></div>
        <div style="width:${channel.complimentary / chTotal * 100}%; background:var(--lifted);"></div>
      </div>
      <div style="display:flex; gap:22px; margin-top:12px; font-size:12.5px; color:var(--ink-soft);">
        <span class="apill direct">Online ${num(channel.online)}</span>
        <span class="apill influenced">Door ${num(channel.door)}</span>
        <span class="apill unknown">Complimentary ${num(channel.complimentary)}</span>
        ${channel.manual ? `<span class="apill unknown">Manual ${num(channel.manual)}</span>` : ""}
      </div>
    </div>

    <section class="row split">
      <div class="panel">
        <div class="panel-head"><div><h2>Marketing</h2><div class="hint">Campaigns linked to this event</div></div></div>
        <div class="lb">
          ${(e.campaigns || []).length ? e.campaigns.map((c) => `
            <div class="lb-row" style="grid-template-columns:1fr auto;">
              <div><div class="lb-name">${esc(c.campaign_name)}</div><div class="lb-sub">Boosted campaign</div></div>
              <div class="lb-score"><span class="s" style="font-size:17px;">${zar(c.spend_cents)}</span></div>
            </div>`).join("") : `<div class="state"><p>No campaigns linked.</p></div>`}
        </div>
      </div>
      <div class="panel">
        <div class="panel-head"><div><h2>Content</h2><div class="hint">Instagram posts for this event</div></div></div>
        <div class="lb">
          ${(e.posts || []).length ? e.posts.map((p) => `
            <div class="lb-row" style="grid-template-columns:auto 1fr auto;">
              <span class="ptype ${(p.post_type || "image").toLowerCase()}">${esc(p.post_type)}</span>
              <div><div class="lb-sub">${num(p.reach)} reach</div></div>
              <div class="lb-score"><span class="s" style="font-size:16px;">${p.engagement_rate != null ? pct(p.engagement_rate, 1) : "—"}</span><span class="o">eng.</span></div>
            </div>`).join("") : `<div class="state"><p>No posts linked.</p></div>`}
        </div>
      </div>
    </section>
  `;

  root.querySelector("#back").addEventListener("click", () => { location.hash = "#/events"; });
  if (velocity.length) drawAreaChart(root, "velocity", velocity, { unit: "tickets" });
}

function kpi(label, value, trust, ctx) {
  return `
    <div class="kpi">
      <div class="top"><span class="label">${label}</span><span class="trust ${trust}">${trust === "calculated" ? "Calc" : "Actual"}</span></div>
      <div class="value">${value}</div>
      <div class="ctx">${ctx}</div>
    </div>`;
}
function money(cents) { return `<span class="cur">R</span>${num(Math.round(cents / 100))}`; }
