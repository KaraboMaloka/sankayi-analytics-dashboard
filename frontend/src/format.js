// Formatting helpers. Money arrives from the API as ZAR *cents* (integers);
// these are the single place we turn wire values into display strings.

const grouper = new Intl.NumberFormat("en-US"); // comma thousands, matches the brand mockups

/** Whole-rand, no decimals: 10988000 -> "R109,880". */
export function zar(cents) {
  return "R" + grouper.format(Math.round((cents ?? 0) / 100));
}

/** Rand with 2 decimals: 3285 -> "R32.85". */
export function zarPrecise(cents) {
  const v = (cents ?? 0) / 100;
  return "R" + v.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
}

/** Plain integer with thousands separators: 14020 -> "14,020". */
export function num(n) {
  return grouper.format(Math.round(n ?? 0));
}

/** Ratio (0..1) to percent: 0.784 -> "78%" (or with decimals). */
export function pct(ratio, decimals = 0) {
  return ((ratio ?? 0) * 100).toFixed(decimals) + "%";
}

/** Multiplier: 7.47 -> "7.47×". */
export function mult(x, decimals = 2) {
  return (x ?? 0).toFixed(decimals) + "×";
}

/** Signed percent for deltas: 0.159 -> "+15.9%". */
export function delta(ratio, decimals = 1) {
  const p = (ratio ?? 0) * 100;
  return (p >= 0 ? "+" : "") + p.toFixed(decimals) + "%";
}

/** "2026-09-20" -> "20 Sep 2026". */
export function niceDate(iso) {
  if (!iso) return "";
  const d = new Date(iso.length <= 10 ? iso + "T00:00:00" : iso);
  return d.toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric" });
}

/** Short date without year: "20 Sep". */
export function shortDate(iso) {
  if (!iso) return "";
  const d = new Date(iso.length <= 10 ? iso + "T00:00:00" : iso);
  return d.toLocaleDateString("en-GB", { day: "numeric", month: "short" });
}

/** Escape user/content strings before inserting into innerHTML. */
export function esc(s) {
  return String(s ?? "").replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c])
  );
}
