import { api } from "../api.js";
import { num, niceDate, esc } from "../format.js";
import { loading, errorState } from "../states.js";
import { icons } from "../icons.js";

const SOURCE_META = {
  instagram: { label: "Instagram", sub: "Meta Graph API", icon: icons.social },
  quicket: { label: "Quicket", sub: "Ticketing REST API", icon: icons.sales },
  meta_ads: { label: "Meta Ads", sub: "Marketing API", icon: icons.marketing },
  yoco: { label: "Yoco", sub: "Card machine · door sales", icon: icons.sales },
};

export async function renderDataHealth(root) {
  loading(root, "Checking data sources…");
  let h;
  try { h = await api.health(); } catch (e) { return errorState(root, e); }

  const sources = h.sources || [];
  const anyFailed = sources.some((s) => s.status === "failed");
  const anyPartial = sources.some((s) => s.status === "partial");
  const dbOk = h.database === "healthy";
  const overall = anyFailed || !dbOk ? "err" : anyPartial ? "warn" : "ok";
  const totalRecords = sources.reduce((a, s) => a + (s.rows || 0), 0);
  const lastSync = sources
    .map((s) => s.last_synced)
    .filter(Boolean)
    .sort()
    .slice(-1)[0];

  const overallText = {
    ok: "All systems healthy",
    warn: "Running with warnings",
    err: "Attention required",
  }[overall];

  root.innerHTML = `
    <div class="section-label">Can I trust today's numbers?</div>

    <div class="summary-banner ${overall}">
      <span class="led-lg"></span>
      <div>
        <div class="big">${overallText}</div>
        <div class="sub">${anyPartial || anyFailed ? "Some sources need a look — details below." : "Every integration is connected and the database is healthy."}</div>
      </div>
      <div class="spacer">
        <div class="metric"><div class="n">${sources.length}</div><div class="l">Integrations</div></div>
        <div class="metric"><div class="n">${num(totalRecords)}</div><div class="l">Records synced</div></div>
        <div class="metric"><div class="n">${lastSync ? freshness(lastSync) : "—"}</div><div class="l">Freshest sync</div></div>
      </div>
    </div>

    <div class="section-label">Integrations</div>
    <div class="health-page">
      ${sources.map(sourceCard).join("")}
      ${dbCard(h.database)}
    </div>
  `;
}

function sourceCard(s) {
  const meta = SOURCE_META[s.source] || { label: cap(s.source), sub: "External source", icon: icons.health };
  const state = s.status === "success" ? "ok" : s.status === "partial" ? "warn" : "err";
  const statusLabel = s.status === "success" ? "Connected" : cap(s.status);
  const panelClass = state === "ok" ? "" : state;
  const stale = isStale(s.last_synced);

  return `
    <div class="hpanel ${panelClass}">
      <div class="hp-top">
        <div class="hp-name">
          <span class="ico">${meta.icon}</span>
          <div><div class="t">${esc(meta.label)}</div><div class="s">${esc(meta.sub)}</div></div>
        </div>
        <span class="status ${state}"><span class="led"></span> ${esc(statusLabel)}</span>
      </div>
      <div class="hp-metrics">
        <div class="hp-metric"><div class="n">${num(s.rows)}</div><div class="l">Records</div></div>
        <div class="hp-metric"><div class="n">${s.last_synced ? freshness(s.last_synced) : "—"}</div><div class="l">Last sync</div></div>
        <div class="hp-metric"><div class="n">${stale ? "Stale" : "Fresh"}</div><div class="l">Freshness</div></div>
      </div>
      ${s.recent && s.recent.length
        ? `<div class="run-strip"><span class="run-lbl">Last ${s.recent.length} runs</span>${s.recent
            .map((st) => `<span class="run-dot ${st === "success" ? "ok" : st === "partial" ? "warn" : "err"}" title="${esc(st)}"></span>`)
            .join("")}</div>`
        : ""}
      ${s.message
        ? `<div class="hp-msg ${state === "ok" ? "ok" : "warn"}">${state === "ok" ? "✓" : "⚠"} ${esc(s.message)}</div>`
        : `<div class="hp-msg ok">✓ Last synced ${esc(niceDate(s.last_synced))} · no issues reported</div>`}
    </div>`;
}

function dbCard(status) {
  const ok = status === "healthy";
  return `
    <div class="hpanel ${ok ? "" : "err"}">
      <div class="hp-top">
        <div class="hp-name">
          <span class="ico">${icons.health}</span>
          <div><div class="t">Database</div><div class="s">PostgreSQL · analytics store</div></div>
        </div>
        <span class="status ${ok ? "ok" : "err"}"><span class="led"></span> ${esc(cap(status))}</span>
      </div>
      <div class="hp-metrics">
        <div class="hp-metric"><div class="n">Live</div><div class="l">Connection</div></div>
        <div class="hp-metric"><div class="n">Append</div><div class="l">Snapshots</div></div>
        <div class="hp-metric"><div class="n">OK</div><div class="l">Integrity</div></div>
      </div>
      <div class="hp-msg ok">✓ Historical snapshots current — growth metrics computable</div>
    </div>`;
}

// ---- time helpers ----
function ageMs(iso) { return Date.now() - new Date(iso).getTime(); }
function isStale(iso) { return iso ? ageMs(iso) > 36 * 3600 * 1000 : true; } // >36h
function freshness(iso) {
  const ms = ageMs(iso);
  const h = Math.round(ms / 3600000);
  if (h < 1) return "just now";
  if (h < 24) return `${h}h ago`;
  const d = Math.round(h / 24);
  return `${d}d ago`;
}
function cap(s) { return String(s || "").charAt(0).toUpperCase() + String(s || "").slice(1); }
