import { api } from "../api.js";
import { esc, niceDate } from "../format.js";

const SOURCE_LABEL = { instagram: "Instagram", quicket: "Quicket", meta_ads: "Meta Ads" };

export async function renderSettings(root) {
  let health = null, me = null;
  try { [health, me] = await Promise.all([api.health(), api.me()]); } catch { /* degrade */ }

  root.innerHTML = `
    <div class="section-label">Account &amp; configuration</div>
    <div class="set-grid">
      <div class="panel">
        <div class="panel-head"><div><h2>Profile</h2><div class="hint">The signed-in user</div></div></div>
        <div class="set-row"><span class="sk">Name</span><span class="sv">${esc(me?.display_name || "—")}</span></div>
        <div class="set-row"><span class="sk">Username</span><span class="sv">@${esc(me?.username || "—")}</span></div>
        <div class="set-row"><span class="sk">Organisation</span><span class="sv">Sankayi Jazz Series</span></div>
      </div>

      <div class="panel">
        <div class="panel-head"><div><h2>Change password</h2><div class="hint">At least 8 characters</div></div></div>
        <form id="pw-form" class="set-form" autocomplete="off">
          <label class="auth-field"><span>Current password</span><input name="current" type="password" autocomplete="current-password" /></label>
          <label class="auth-field"><span>New password</span><input name="newpw" type="password" autocomplete="new-password" /></label>
          <label class="auth-field"><span>Confirm new password</span><input name="confirmpw" type="password" autocomplete="new-password" /></label>
          <div class="auth-error" id="pw-err" style="display:none"></div>
          <div class="auth-note" id="pw-ok" style="display:none"></div>
          <button type="submit" class="auth-btn">Update password</button>
        </form>
      </div>

      <div class="panel set-full">
        <div class="panel-head"><div><h2>Integrations</h2><div class="hint">Connected data sources</div></div></div>
        ${(health?.sources || []).map(intRow).join("") || `<div class="set-row"><span class="sk">Status unavailable</span><span class="sv mut">API offline</span></div>`}
        <div class="set-row"><span class="sk">Database</span><span class="sv">${health ? esc(cap(health.database)) : "—"} · PostgreSQL</span></div>
      </div>
    </div>
  `;

  const form = root.querySelector("#pw-form");
  const err = root.querySelector("#pw-err");
  const ok = root.querySelector("#pw-ok");
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    err.style.display = ok.style.display = "none";
    const cur = form.current.value, np = form.newpw.value, cf = form.confirmpw.value;
    const fail = (m) => { err.textContent = m; err.style.display = "block"; };
    if (np.length < 8) return fail("New password must be at least 8 characters.");
    if (np !== cf) return fail("New passwords don't match.");
    const btn = form.querySelector("button");
    btn.disabled = true; btn.textContent = "…";
    try {
      await api.changePassword(cur, np);
      form.reset();
      ok.textContent = "Password updated."; ok.style.display = "block";
    } catch (ex) {
      fail(ex.message || "Couldn't update password.");
    } finally {
      btn.disabled = false; btn.textContent = "Update password";
    }
  });
}

function intRow(s) {
  const connected = s.status === "success";
  const label = connected ? "Connected" : cap(s.status);
  return `
    <div class="set-row">
      <span class="sk">${esc(SOURCE_LABEL[s.source] || cap(s.source))}</span>
      <span class="sv" style="display:flex; align-items:center; gap:14px;">
        <span class="status ${connected ? "ok" : s.status === "partial" ? "warn" : "err"}"><span class="led"></span> ${esc(label)}</span>
        <span class="sv mut" style="font-size:12px;">synced ${s.last_synced ? esc(niceDate(s.last_synced)) : "—"}</span>
      </span>
    </div>`;
}

function cap(s) { return String(s || "").charAt(0).toUpperCase() + String(s || "").slice(1); }
