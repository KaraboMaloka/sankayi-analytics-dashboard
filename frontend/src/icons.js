// Inline SVG icon strings (stroke uses currentColor).
const s = (p) => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${p}</svg>`;

export const icons = {
  overview: s(`<rect x="3" y="3" width="7" height="9" rx="1.5"/><rect x="14" y="3" width="7" height="5" rx="1.5"/><rect x="14" y="12" width="7" height="9" rx="1.5"/><rect x="3" y="16" width="7" height="5" rx="1.5"/>`),
  events: s(`<rect x="3" y="4" width="18" height="17" rx="2"/><path d="M3 9h18M8 2v4M16 2v4"/>`),
  sales: s(`<path d="M3 9a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v1a2 2 0 0 0 0 4v1a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-1a2 2 0 0 0 0-4z"/><path d="M13 7v10"/>`),
  artists: s(`<circle cx="12" cy="8" r="4"/><path d="M4 21a8 8 0 0 1 16 0"/>`),
  social: s(`<rect x="3" y="3" width="18" height="18" rx="5"/><circle cx="12" cy="12" r="4"/><circle cx="17.5" cy="6.5" r="1.2" fill="currentColor"/>`),
  marketing: s(`<path d="M3 3v18h18"/><path d="M7 14l4-4 3 3 5-6"/>`),
  forecasts: s(`<path d="M12 3v4M12 21a8 8 0 1 0-8-8"/><path d="M12 12l4-2"/>`),
  reports: s(`<path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><path d="M14 3v6h6M8 13h8M8 17h5"/>`),
  health: s(`<ellipse cx="12" cy="5" rx="8" ry="3"/><path d="M4 5v14c0 1.7 3.6 3 8 3s8-1.3 8-3V5M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3"/>`),
  settings: s(`<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-2.9 1.2V22a2 2 0 1 1-4 0v-.1A1.7 1.7 0 0 0 6.9 20l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1A1.7 1.7 0 0 0 4 15M20 9a1.7 1.7 0 0 0 1-1.6V7a2 2 0 1 0-4 0"/>`),
  logout: s(`<path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><path d="M16 17l5-5-5-5M21 12H9"/>`),
  search: s(`<circle cx="11" cy="11" r="7"/><path d="M21 21l-4-4"/>`),
  bell: s(`<path d="M18 8a6 6 0 1 0-12 0c0 7-3 9-3 9h18s-3-2-3-9"/><path d="M13.7 21a2 2 0 0 1-3.4 0"/>`),
  chevron: s(`<path d="M6 9l6 6 6-6"/>`),
};
