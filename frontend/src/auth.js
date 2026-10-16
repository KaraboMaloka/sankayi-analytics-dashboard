// Session token storage (per-browser). The token is an opaque server-issued
// string; the backend maps it to the user and can revoke it on logout.

const TOKEN_KEY = "sankayi_token";

export function getToken() {
  try { return localStorage.getItem(TOKEN_KEY); } catch { return null; }
}
export function setToken(t) {
  try { localStorage.setItem(TOKEN_KEY, t); } catch { /* storage blocked */ }
}
export function clearToken() {
  try { localStorage.removeItem(TOKEN_KEY); } catch { /* storage blocked */ }
}
