// Thin client over the Rust API. In dev, Vite proxies /api -> :8080, so these
// are same-origin requests. Everything returns parsed JSON or throws.

import { getToken } from "./auth.js";

const BASE = "/api";

async function request(method, path, { body, auth = true } = {}) {
  const headers = { Accept: "application/json" };
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const token = getToken();
  if (auth && token) headers.Authorization = `Bearer ${token}`;

  let res;
  try {
    res = await fetch(BASE + path, {
      method,
      headers,
      cache: "no-store", // analytics data changes with every ingest
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });
  } catch (e) {
    throw new ApiError("network", "Couldn't reach the Sankayi API. Is the server running on :8080?");
  }
  if (!res.ok) {
    let payload;
    try { payload = await res.json(); } catch { /* ignore */ }
    const code = payload?.error?.code || (res.status === 401 ? "unauthorized" : "http_" + res.status);
    const msg = payload?.error?.message || `Request failed (${res.status})`;
    throw new ApiError(code, msg);
  }
  return res.json();
}

const get = (path) => request("GET", path);

export class ApiError extends Error {
  constructor(code, message) {
    super(message);
    this.code = code;
  }
}

export const api = {
  // --- auth ---
  login: (username, password) => request("POST", "/login", { body: { username, password }, auth: false }),
  logout: () => request("POST", "/logout"),
  me: () => get("/me"),
  changePassword: (current_password, new_password) =>
    request("POST", "/change-password", { body: { current_password, new_password } }),
  forgotPassword: (username, new_password) =>
    request("POST", "/forgot-password", { body: { username, new_password }, auth: false }),

  // --- data ---
  dashboard: () => get("/dashboard"),
  events: () => get("/events"),
  event: (id) => get("/events/" + encodeURIComponent(id)),
  artists: () => get("/artists"),
  artist: (id) => get("/artists/" + encodeURIComponent(id)),
  social: () => get("/social"),
  campaigns: () => get("/campaigns"),
  sales: (params = {}) => {
    const q = new URLSearchParams(params).toString();
    return get("/sales" + (q ? "?" + q : ""));
  },
  forecasts: () => get("/forecasts"),
  health: () => get("/health"),
};
