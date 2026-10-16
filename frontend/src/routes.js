import { icons } from "./icons.js";
import { renderOverview } from "./pages/overview.js";
import { renderMarketing } from "./pages/marketing.js";
import { renderArtists } from "./pages/artists.js";
import { renderSocial } from "./pages/social.js";
import { renderDataHealth } from "./pages/datahealth.js";
import { renderEvents } from "./pages/events.js";
import { renderReports } from "./pages/reports.js";
import { renderSettings } from "./pages/settings.js";

// Order here is the sidebar order.
export const ROUTES = [
  { id: "overview",  label: "Overview",     icon: icons.overview,  eyebrow: "Command Centre",      title: "Overview",     render: renderOverview, filters: true },
  { id: "events",    label: "Events",       icon: icons.events,    eyebrow: "Event Performance",   title: "Events",       render: renderEvents, filters: true },
  { id: "artists",   label: "Artists",      icon: icons.artists,   eyebrow: "Artist Intelligence", title: "Artists",      render: renderArtists },
  { id: "social",    label: "Social Media", icon: icons.social,    eyebrow: "Audience",            title: "Social Media", render: renderSocial },
  { id: "marketing", label: "Marketing",    icon: icons.marketing, eyebrow: "Campaign Intelligence", title: "Marketing",  render: renderMarketing },
  { id: "reports",   label: "Reports",      icon: icons.reports,   eyebrow: "Exports",             title: "Reports",      render: renderReports },
  { id: "health",    label: "Data Health",  icon: icons.health,    eyebrow: "Data Trust",          title: "Data Health",  render: renderDataHealth },
  { id: "settings",  label: "Settings",     icon: icons.settings,  eyebrow: "Configuration",       title: "Settings",     render: renderSettings },
];

export const DEFAULT_ROUTE = "overview";
export const routeById = (id) => ROUTES.find((r) => r.id === id);
