import { api } from "../api.js";
import { zar, zarPrecise, num, pct, mult, niceDate, esc } from "../format.js";
import { loading, errorState } from "../states.js";

export async function renderMarketing(root) {
  loading(root, "Loading campaign intelligence…");

  let camp, dash;
  try {
    [camp, dash] = await Promise.all([api.campaigns(), api.dashboard()]);
  } catch (e) {
    return errorState(root, e);
  }

  const cs = camp.campaigns;
  if (cs.length === 0) {
    root.innerHTML = `<div class="panel"><div class="state"><h3>No campaigns yet</h3><p>Once boosted posts are running, their spend and attribution appear here.</p></div></div>`;
    return;
  }

  // Totals for the funnel + KPIs.
  const sum = (f) => cs.reduce((a, c) => a + (f(c) || 0), 0);
  const spend = sum((c) => c.spend_cents);
  const reach = sum((c) => c.reach);
  const engagements = sum((c) => c.engagements);
  const clicks = sum((c) => c.clicks);
  const tickets = sum((c) => c.attributed_tickets);
  const revenue = sum((c) => c.attributed_revenue_cents);
  const roas = spend ? revenue / spend : 0;
  const costPerTicket = tickets ? spend / tickets : 0;
  const avgPrice = tickets ? revenue / tickets : 0;

  const engRate = reach ? engagements / reach : 0;
  const clickRate = engagements ? clicks / engagements : 0;
  const buyRate = clicks ? tickets / clicks : 0;

  const maxRev = Math.max(...cs.map((c) => c.attributed_revenue_cents), 1);
  const maxRoas = Math.max(...cs.map((c) => c.roas || 0), 1);

  // Best & worst by ROAS for the recommendation.
  const ranked = [...cs].sort((a, b) => (b.roas || 0) - (a.roas || 0));
  const best = ranked[0], worst = ranked[ranked.length - 1];

  // Both tables paginate (a season of boosts is a lot to scroll), highest
  // spend first so the campaigns that matter lead.
  const PER_PAGE = 10;
  const display = [...cs].sort((a, b) => b.spend_cents - a.spend_cents);
  const pageCount = Math.max(1, Math.ceil(display.length / PER_PAGE));
  let campPage = 0;
  let srPage = 0;

  const campRowHTML = (c) => `
    <tr>
      <td><div class="c-name">${esc(c.campaign_name)}</div>${c.permalink
        ? `<a class="c-link" href="${esc(c.permalink)}" target="_blank" rel="noopener">↗ View on Instagram</a>`
        : `<div class="c-sub">${esc(c.campaign_tag || "")}</div>`}</td>
      <td>${esc(platformLabel(c.platform))}</td>
      <td>${zar(c.spend_cents)}</td>
      <td>${num(c.reach)}</td>
      <td>${num(c.clicks)}</td>
      <td>${num(c.attributed_tickets)}</td>
      <td>${zar(c.attributed_revenue_cents)}</td>
      <td><div class="roas-cell"><span class="rv">${mult(c.roas || 0)}</span><span class="rb"><i style="width:${Math.min(((c.roas || 0) / maxRoas) * 100, 100)}%"></i></span></div></td>
      <td>${statusPill(c)}</td>
    </tr>`;

  const srItemHTML = (c) => `
    <div class="sr-item">
      <div class="sr-top"><span class="nm">${esc(c.campaign_name)}</span><span class="rz">${mult(c.roas || 0)} return</span></div>
      <div class="sr-bars">
        <div class="sr-bar"><span class="k">Spend</span><div class="track"><div class="fill spend" style="width:${Math.max((c.spend_cents / maxRev) * 100, 2)}%"></div></div><span class="amt">${zar(c.spend_cents)}</span></div>
        <div class="sr-bar"><span class="k">Revenue</span><div class="track"><div class="fill rev" style="width:${Math.max((c.attributed_revenue_cents / maxRev) * 100, 2)}%"></div></div><span class="amt">${zar(c.attributed_revenue_cents)}</span></div>
      </div>
    </div>`;

  root.innerHTML = `
    <div class="section-label">Are we spending our marketing money effectively?</div>

    <section class="kpi-grid cols-4">
      ${kpi("Total Ad Spend", money(spend), "actual", `${cs.length} boosted campaigns`)}
      ${kpi("Reach", num(reach), "actual", "unique accounts")}
      ${kpi("Engagement", num(engagements), "actual", `${pct(engRate, 1)} of reach`)}
      ${kpi("Link Clicks", num(clicks), "actual", "to Quicket")}
      ${kpi("Tickets Attributed", num(tickets), "calculated", "direct + influenced")}
      ${kpi("Revenue Attributed", money(revenue), "calculated", `of ${zar(dash.revenue_cents)} total`)}
      ${kpi("Blended ROAS", mult(roas), "calculated", "revenue ÷ spend")}
      ${kpi("Cost / Ticket", zarPrecise(costPerTicket), "calculated", "spend ÷ tickets")}
    </section>

    <div class="info-note" id="utm-note">
      <span class="in-ic">ℹ</span>
      <div class="in-body">
        <b>Tickets, revenue & ROAS below are estimated, not tracked.</b> Quicket doesn't report which ticket came from which ad, so we model it from each boost's timing and clicks. Spend, reach and link-clicks are exact (from Meta) — the ticket/revenue/ROAS figures are a well-reasoned estimate.
        <div class="in-tip"><b>Tracking is now wired up.</b> Give each boost a unique <b>Quicket promo code</b>, then register it (campaign&nbsp;→&nbsp;code). Any sale that uses the code is credited <b>directly</b> to that boost — proven, not modeled — and its ROAS becomes exact. (A link/UTM alone can't do this: Quicket only tells us the promo code a buyer used, never the ad they clicked.)</div>
      </div>
      <button class="in-x" id="utm-note-x" aria-label="Dismiss">✕</button>
    </div>

    <section class="row funnel-row">
      <div class="panel">
        <div class="panel-head"><div><h2>Attribution Funnel</h2><div class="hint">How ad spend converts into ticket revenue · all campaigns</div></div></div>
        <div class="funnel">
          <div class="f-cap"><span class="cl">Input · Ad Spend</span><span class="cv">${zar(spend)}</span></div>
          <div class="f-conv"><span class="arrow">↓</span> reached <span class="pc">${num(reach)}</span> accounts</div>
          ${stage("Reach", num(reach), 100, "linear-gradient(90deg,#F8894A,#EC6A2C)")}
          <div class="f-conv"><span class="arrow">↓</span> <span class="pc">${pct(engRate, 1)}</span> engaged</div>
          ${stage("Engagement", num(engagements), 68, "linear-gradient(90deg,#EC6A2C,#D65E27)")}
          <div class="f-conv"><span class="arrow">↓</span> <span class="pc">${pct(clickRate, 1)}</span> clicked through</div>
          ${stage("Link Clicks", num(clicks), 46, "linear-gradient(90deg,#D65E27,#BE5321)")}
          <div class="f-conv"><span class="arrow">↓</span> <span class="pc">${pct(buyRate, 1)}</span> purchased</div>
          ${stage("Tickets", num(tickets), 32, "linear-gradient(90deg,#BE5321,#A94D20)", "#fff")}
          <div class="f-conv"><span class="arrow">↓</span> at ${zar(avgPrice)} average</div>
          <div class="f-cap out"><span class="cl">Output · Attributed Revenue</span><span class="cv">${zar(revenue)} <small>${mult(roas)} ROAS</small></span></div>
        </div>
      </div>

      <div class="panel reco">
        <span class="rlabel">✦ Recommendation</span>
        <h3>${esc(best.campaign_name)} is your most efficient spend — replicate its format.</h3>
        <p>The <b>${esc(best.campaign_name)}</b> returned <b>${mult(best.roas || 0)}</b> on <b>${zar(best.spend_cents)}</b>${worst && worst !== best ? `, well ahead of ${esc(worst.campaign_name)}'s ${mult(worst.roas || 0)}` : ""}. Its click-to-ticket rate ${best.ticket_conversion != null ? `of <b>${pct(best.ticket_conversion, 1)}</b> ` : ""}beat the ${pct(buyRate, 1)} blended rate.</p>
        <p style="margin-top:12px;">Leaning the next rand toward that creative style is where spend works hardest.</p>
        ${(() => {
          const tops = ranked.filter((c) => (c.roas || 0) > 0).slice(0, 3);
          const mx = Math.max(...tops.map((c) => c.roas || 0), 1);
          return tops.length
            ? `<div class="viz-cap">Best campaigns by return</div>${miniBars(tops.map((c) => ({ label: esc(c.campaign_name), pct: (c.roas || 0) / mx, value: mult(c.roas || 0) })))}`
            : "";
        })()}
        <div class="disclaim">Analytical suggestion based on historical performance — not a guarantee.</div>
      </div>
    </section>

    <div class="section-label">Campaign performance</div>
    <div class="panel">
      <div class="tbl-wrap">
        <table class="camp">
          <thead><tr>
            <th>Campaign / Event</th><th>Platform</th><th>Spend</th><th>Reach</th>
            <th>Clicks</th><th>Tickets</th><th>Revenue</th><th>ROAS</th><th>Status</th>
          </tr></thead>
          <tbody id="camp-body">
            ${display.slice(0, PER_PAGE).map(campRowHTML).join("")}
          </tbody>
        </table>
      </div>
      <div class="pager">
        <button id="cp-prev" class="pg-btn" disabled>← Prev</button>
        <span id="cp-ind" class="pg-ind">Page 1 of ${pageCount}</span>
        <button id="cp-next" class="pg-btn"${pageCount <= 1 ? " disabled" : ""}>Next →</button>
      </div>
    </div>

    <div class="section-label">Spend vs attributed revenue</div>
    <div class="panel">
      <div class="panel-head"><div><h2>Return by Campaign</h2><div class="hint">Every rand of spend against the revenue it brought back · scaled to the top earner</div></div></div>
      <div class="sr-list" id="sr-body">
        ${display.slice(0, PER_PAGE).map(srItemHTML).join("")}
      </div>
      <div class="pager">
        <button id="sr-prev" class="pg-btn" disabled>← Prev</button>
        <span id="sr-ind" class="pg-ind">Page 1 of ${pageCount}</span>
        <button id="sr-next" class="pg-btn"${pageCount <= 1 ? " disabled" : ""}>Next →</button>
      </div>
    </div>
  `;

  // ---- pagination wiring (both tables share the same page size) ----
  wirePager(root, "cp", "camp-body", display, campRowHTML, pageCount, () => campPage, (v) => (campPage = v));
  wirePager(root, "sr", "sr-body", display, srItemHTML, pageCount, () => srPage, (v) => (srPage = v));

  // ---- UTM guidance note: dismissible, remembered per browser ----
  const note = root.querySelector("#utm-note");
  try {
    if (localStorage.getItem("hideUtmNote") === "1") note?.remove();
  } catch { /* storage unavailable — just show it */ }
  root.querySelector("#utm-note-x")?.addEventListener("click", () => {
    try { localStorage.setItem("hideUtmNote", "1"); } catch { /* ignore */ }
    note?.remove();
  });
}

// Generic prev/next pager: re-renders `container` with the current page slice.
function wirePager(root, prefix, containerId, items, rowFn, pageCount, getPage, setPage) {
  const PER = 10;
  const container = root.querySelector(`#${containerId}`);
  const prev = root.querySelector(`#${prefix}-prev`);
  const next = root.querySelector(`#${prefix}-next`);
  const ind = root.querySelector(`#${prefix}-ind`);
  if (!container || !prev || !next) return;
  function render() {
    const p = getPage();
    const start = p * PER;
    container.innerHTML = items.slice(start, start + PER).map(rowFn).join("");
    ind.textContent = `Page ${p + 1} of ${pageCount}`;
    prev.disabled = p === 0;
    next.disabled = p >= pageCount - 1;
  }
  prev.addEventListener("click", () => { if (getPage() > 0) { setPage(getPage() - 1); render(); } });
  next.addEventListener("click", () => { if (getPage() < pageCount - 1) { setPage(getPage() + 1); render(); } });
}

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

function kpi(label, value, trust, ctx) {
  return `
    <div class="kpi">
      <div class="top"><span class="label">${label}</span><span class="trust ${trust}">${trust === "calculated" ? "Calc" : "Actual"}</span></div>
      <div class="value">${value}</div>
      <div class="ctx">${ctx}</div>
    </div>`;
}

function stage(name, val, width, bg, color) {
  return `<div class="f-stage" style="width:${width}%; background:${bg};${color ? "color:" + color + ";" : ""}"><span class="fs-name">${name}</span><span class="fs-val">${val}</span></div>`;
}

function statusPill(c) {
  // Heuristic: a campaign whose window has closed reads "Completed"; else "Active".
  const active = c.end_date ? new Date(c.end_date) >= new Date() : false;
  return active
    ? `<span class="pill active"><span class="d"></span> Active</span>`
    : `<span class="pill done"><span class="d"></span> Completed</span>`;
}

function platformLabel(p) {
  return { meta_ads: "Meta Ads", instagram: "Instagram", facebook: "Facebook", google: "Google", other: "Other" }[p] || p;
}
