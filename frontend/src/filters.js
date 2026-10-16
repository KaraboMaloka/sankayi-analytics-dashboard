// Global topbar filter state: period + event + search. Pages read this and
// filter client-side (the dataset is small). Subscribers re-render on change.

const state = { period: "all", eventId: "", search: "" };
const listeners = new Set();

export const PERIOD_LABELS = {
  all: "All time",
  "7d": "Last 7 days",
  "30d": "Last 30 days",
  "90d": "Last 90 days",
};

export const filters = {
  get: () => ({ ...state }),
  set(patch) {
    Object.assign(state, patch);
    listeners.forEach((fn) => fn({ ...state }));
  },
  reset() {
    this.set({ period: "all", eventId: "", search: "" });
  },
  subscribe(fn) {
    listeners.add(fn);
    return () => listeners.delete(fn);
  },
  /** True if any filter is narrowing the data. */
  active() {
    return state.period !== "all" || state.eventId !== "" || state.search.trim() !== "";
  },
};

/** Is a date (YYYY-MM-DD or ISO) within the selected period window [now-N, now]? */
export function inPeriod(dateLike, period = state.period) {
  if (!period || period === "all") return true;
  const days = { "7d": 7, "30d": 30, "90d": 90 }[period];
  if (!days) return true;
  if (!dateLike) return false;
  const d = new Date(dateLike.length <= 10 ? dateLike + "T00:00:00" : dateLike);
  const now = new Date();
  const cutoff = new Date();
  cutoff.setDate(now.getDate() - days);
  return d >= cutoff && d <= now;
}

/** Does a name match the free-text search (case-insensitive substring)? */
export function matchesSearch(name, search = state.search) {
  const q = (search || "").trim().toLowerCase();
  if (!q) return true;
  return String(name || "").toLowerCase().includes(q);
}
