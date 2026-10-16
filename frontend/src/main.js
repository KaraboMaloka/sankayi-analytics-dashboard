import "./styles.css";
import logoUrl from "./assets/sankayi-orange.png";
import { icons } from "./icons.js";
import { ROUTES, DEFAULT_ROUTE, routeById } from "./routes.js";
import { filters, PERIOD_LABELS } from "./filters.js";
import { api } from "./api.js";
import { esc } from "./format.js";
import { renderLogin } from "./login.js";
import { getToken, clearToken } from "./auth.js";

let currentUser = null;

function initials(name) {
  return (name || "?").split(/\s+/).filter(Boolean).slice(0, 2).map((w) => w[0].toUpperCase()).join("");
}

let currentId = null;
let eventOptions = []; // {id, name} for the Event dropdown

// ---- shell (built once; the router only swaps #view) ----
function shell() {
  const nav = ROUTES.map(
    (r) => `<a class="nav-item" data-route="${r.id}" href="#/${r.id}">${r.icon} ${r.label}</a>`
  ).join("");

  return `
    <div class="app">
      <aside class="sidebar">
        <div class="brand">
          <div class="logo-disc" style="background-image:url('${logoUrl}')" role="img" aria-label="Sankayi Jazz Series logo"></div>
          <div>
            <div class="mark">SANKAYI<span class="intel">INTELLIGENCE</span></div>
            <div class="sub">Jazz Series</div>
          </div>
        </div>
        <nav class="nav">${nav}</nav>
        <div class="side-footer">
          <div class="user">
            <div class="avatar">${esc(initials(currentUser?.display_name))}</div>
            <div><div class="name">${esc(currentUser?.display_name || "")}</div><div class="role">@${esc(currentUser?.username || "")}</div></div>
          </div>
          <button class="logout" type="button">${icons.logout} Log out</button>
        </div>
      </aside>
      <div class="main">
        <header class="topbar">
          <div class="page-title"><span class="eyebrow"></span><h1></h1></div>
          <div class="filters" id="filters">
            <div class="dropdown" id="dd-period">
              <button class="control" id="btn-period"><span class="cap">Period</span> <span id="lbl-period">All time</span> ${icons.chevron}</button>
              <div class="menu" id="menu-period"></div>
            </div>
            <div class="dropdown" id="dd-event">
              <button class="control" id="btn-event"><span class="cap">Event</span> <span id="lbl-event">All Events</span> ${icons.chevron}</button>
              <div class="menu" id="menu-event"></div>
            </div>
            <div class="search-wrap">
              <input class="search-input" id="search-input" placeholder="Search events…" aria-label="Search events" />
              <button class="icon-btn" id="btn-search" aria-label="Search">${icons.search}</button>
            </div>
            <button class="icon-btn" aria-label="Notifications">${icons.bell}</button>
          </div>
        </header>
        <div class="content" id="view"></div>
      </div>
    </div>`;
}

function currentRouteId() {
  const id = (location.hash || "").replace(/^#\/?/, "").split("?")[0];
  return routeById(id) ? id : DEFAULT_ROUTE;
}

async function renderView() {
  const route = routeById(currentId);
  const view = document.getElementById("view");
  view.scrollTop = 0;
  try {
    await route.render(view);
  } catch (e) {
    // A mid-session token expiry should drop back to the login screen.
    if (e?.code === "unauthorized") { clearToken(); return boot(); }
    view.innerHTML = `<div class="state error"><h3>Failed to render ${route.title}</h3><p>${String(e?.message || e)}</p></div>`;
  }
}

async function navigate() {
  currentId = currentRouteId();
  const route = routeById(currentId);
  document.querySelector(".page-title .eyebrow").textContent = route.eyebrow;
  document.querySelector(".page-title h1").textContent = route.title;
  document.querySelectorAll(".nav-item").forEach((a) =>
    a.classList.toggle("active", a.dataset.route === currentId)
  );
  // Only show the filter controls on pages that honour them.
  const show = !!route.filters;
  document.getElementById("dd-period").style.display = show ? "" : "none";
  document.getElementById("dd-event").style.display = show ? "" : "none";
  document.querySelector(".search-wrap").style.display = show ? "" : "none";
  syncControlLabels();
  await renderView();
}

// ---- filter controls ----
function closeMenus() {
  document.querySelectorAll(".menu.open").forEach((m) => m.classList.remove("open"));
}

function syncControlLabels() {
  const f = filters.get();
  document.getElementById("lbl-period").textContent = PERIOD_LABELS[f.period] || "All time";
  const ev = eventOptions.find((e) => e.id === f.eventId);
  document.getElementById("lbl-event").textContent = ev ? ev.name : "All Events";
  document.getElementById("btn-period").classList.toggle("filtered", f.period !== "all");
  document.getElementById("btn-event").classList.toggle("filtered", !!f.eventId);
  const search = document.getElementById("search-input");
  document.getElementById("btn-search").classList.toggle("filtered", !!f.search.trim());
  if (f.search && !search.classList.contains("open")) search.classList.add("open");
  if (search.value !== f.search) search.value = f.search;
}

function buildPeriodMenu() {
  const menu = document.getElementById("menu-period");
  menu.innerHTML = Object.entries(PERIOD_LABELS)
    .map(([k, label]) => `<button data-period="${k}">${label}</button>`)
    .join("");
  menu.addEventListener("click", (e) => {
    const b = e.target.closest("button[data-period]");
    if (!b) return;
    filters.set({ period: b.dataset.period });
    closeMenus();
  });
}

function buildEventMenu() {
  const menu = document.getElementById("menu-event");
  const items = [`<button data-event="">All Events</button>`, `<div class="menu-label">Shows</div>`]
    .concat(eventOptions.map((e) => `<button data-event="${e.id}" title="${esc(e.name)}">${esc(e.name)}</button>`));
  menu.innerHTML = items.join("");
  menu.addEventListener("click", (e) => {
    const b = e.target.closest("button[data-event]");
    if (!b) return;
    filters.set({ eventId: b.dataset.event });
    closeMenus();
  });
}

async function loadEventOptions() {
  try {
    const data = await api.events();
    eventOptions = data.events
      .map((e) => ({ id: e.id, name: e.artist?.name || e.title, date: e.event_date }))
      .sort((a, b) => (b.date || "").localeCompare(a.date || ""));
  } catch {
    eventOptions = [];
  }
  buildEventMenu();
  syncControlLabels();
}

function wireControls() {
  buildPeriodMenu();
  buildEventMenu();

  const toggle = (menuId) => (e) => {
    e.stopPropagation();
    const menu = document.getElementById(menuId);
    const wasOpen = menu.classList.contains("open");
    closeMenus();
    if (!wasOpen) menu.classList.add("open");
  };
  document.getElementById("btn-period").addEventListener("click", toggle("menu-period"));
  document.getElementById("btn-event").addEventListener("click", toggle("menu-event"));
  document.addEventListener("click", closeMenus);
  document.querySelectorAll(".menu").forEach((m) => m.addEventListener("click", (e) => e.stopPropagation()));

  const input = document.getElementById("search-input");
  document.getElementById("btn-search").addEventListener("click", (e) => {
    e.stopPropagation();
    input.classList.toggle("open");
    if (input.classList.contains("open")) input.focus();
    else if (input.value) { input.value = ""; filters.set({ search: "" }); }
  });
  let t;
  input.addEventListener("input", () => {
    clearTimeout(t);
    t = setTimeout(() => filters.set({ search: input.value }), 200);
  });
  input.addEventListener("keydown", (e) => { if (e.key === "Escape") { input.value = ""; filters.set({ search: "" }); input.classList.remove("open"); } });
}

let wired = false;

async function boot() {
  // Gate on a valid session: validate any stored token, else show login.
  currentUser = null;
  const token = getToken();
  if (token) {
    try { currentUser = await api.me(); } catch { clearToken(); }
  }
  if (!currentUser) {
    renderLogin(() => boot());
    return;
  }

  document.getElementById("app").innerHTML = shell();
  wireControls();
  document.querySelector(".logout").addEventListener("click", async () => {
    try { await api.logout(); } catch { /* revoke best-effort */ }
    clearToken();
    boot();
  });

  // Wire cross-cutting listeners once (they reference #view/controls by id,
  // which the shell recreates on each boot, so re-attaching is safe but noisy).
  if (!wired) {
    wired = true;
    filters.subscribe(() => {
      if (!currentUser) return;
      syncControlLabels();
      renderView();
    });
    window.addEventListener("hashchange", () => { if (currentUser) navigate(); });
  }

  if (!location.hash) location.hash = "#/" + DEFAULT_ROUTE;
  navigate();
  loadEventOptions();
}

boot();
