import { api } from "../api.js";
import { pct } from "../format.js";
import { downloadCsv } from "../csv.js";
import { downloadXlsx, downloadPdf } from "../exports.js";
import { icons } from "../icons.js";

// Each report knows how to fetch its data and shape it into CSV rows. CSV export
// is genuinely functional (live data); PDF/Excel are flagged as later-phase.
const REPORTS = {
  events: {
    title: "Event Performance",
    desc: "Every Jazz Series night — tickets, revenue, sell-through and status.",
    file: "sankayi-event-performance.csv",
    async build() {
      const d = await api.events();
      const headers = ["Event", "Artist", "Date", "Status", "Capacity", "Tickets Sold", "Revenue (ZAR)", "Sell-Through"];
      const rows = d.events.map((e) => [
        e.title, e.artist?.name || "", e.event_date, e.status, e.capacity,
        e.tickets_sold, (e.revenue_cents / 100).toFixed(2), pct(e.sell_through),
      ]);
      return { headers, rows };
    },
  },
  ticketsales: {
    title: "Ticket Sales",
    desc: "Line-level sales feed — every order with type, quantity, price, source and attribution.",
    file: "sankayi-ticket-sales.csv",
    async build() {
      const [sales, events] = await Promise.all([api.sales({ pageSize: 20000 }), api.events()]);
      const nameOf = new Map(events.events.map((e) => [e.id, e.artist?.name || e.title]));
      const headers = ["Sold At", "Event", "Type", "Quantity", "Price (ZAR)", "Source", "Attribution"];
      const rows = (sales.sales || [])
        .slice()
        .sort((a, b) => (b.sold_at || "").localeCompare(a.sold_at || ""))
        .map((s) => [
          s.sold_at, nameOf.get(s.event_id) || "", s.ticket_type, s.quantity,
          (s.price_cents / 100).toFixed(2), s.sale_source, s.attribution,
        ]);
      return { headers, rows };
    },
  },
  artists: {
    title: "Artist Performance",
    desc: "Leaderboard with tickets, revenue, sell-through, engagement, ROI and score.",
    file: "sankayi-artist-performance.csv",
    async build() {
      const d = await api.artists();
      const headers = ["Artist", "Genre", "Events", "Tickets", "Revenue (ZAR)", "Avg Sell-Through", "Engagement", "ROI", "Score"];
      const rows = d.artists.map((a) => [
        a.name, a.genre || "", a.events_count, a.tickets_sold, (a.revenue_cents / 100).toFixed(2),
        pct(a.avg_sell_through), pct(a.avg_engagement_rate, 2), a.roi.toFixed(2), a.performance_score.toFixed(1),
      ]);
      return { headers, rows };
    },
  },
  marketing: {
    title: "Marketing ROI",
    desc: "Campaign spend, reach, clicks, attributed tickets & revenue, and ROAS.",
    file: "sankayi-marketing-roi.csv",
    async build() {
      const d = await api.campaigns();
      const headers = ["Campaign", "Tag", "Platform", "Spend (ZAR)", "Reach", "Clicks", "Tickets", "Revenue (ZAR)", "ROAS"];
      const rows = d.campaigns.map((c) => [
        c.campaign_name, c.campaign_tag || "", c.platform, (c.spend_cents / 100).toFixed(2),
        c.reach, c.clicks, c.attributed_tickets, (c.attributed_revenue_cents / 100).toFixed(2),
        (c.roas || 0).toFixed(2),
      ]);
      return { headers, rows };
    },
  },
  social: {
    title: "Social Media",
    desc: "Top-performing posts with reach and full engagement breakdown.",
    file: "sankayi-social-posts.csv",
    async build() {
      const d = await api.social();
      const headers = ["Post", "Type", "Published", "Reach", "Likes", "Comments", "Shares", "Saves", "Engagement"];
      const rows = (d.top_posts || []).map((p) => [
        p.caption || p.event_title || "", p.post_type, p.published_at, p.reach,
        p.likes, p.comments, p.shares, p.saves, pct(p.engagement_rate, 2),
      ]);
      return { headers, rows };
    },
  },
  summary: {
    title: "Monthly Performance",
    desc: "Executive summary — tickets, revenue, sell-through, followers, spend, ROAS.",
    file: "sankayi-summary.csv",
    async build() {
      const d = await api.dashboard();
      const headers = ["Metric", "Value"];
      const rows = [
        ["Tickets sold", d.tickets_sold],
        ["Revenue (ZAR)", (d.revenue_cents / 100).toFixed(2)],
        ["Avg sell-through", pct(d.sell_through)],
        ["Followers", d.followers],
        ["Follower growth", pct(d.followers_growth, 1)],
        ["Ad spend (ZAR)", (d.ad_spend_cents / 100).toFixed(2)],
        ["Blended ROAS", d.roas.toFixed(2)],
      ];
      return { headers, rows };
    },
  },
};

export function renderReports(root) {
  root.innerHTML = `
    <div class="section-label">Generate a report</div>
    <div class="report-grid">
      ${Object.entries(REPORTS).map(([key, r]) => `
        <div class="report-card">
          <div class="r-top">
            <span class="r-ico">${icons.reports}</span>
            <div class="r-title">${r.title}</div>
          </div>
          <div class="r-desc">${r.desc}</div>
          <div class="export-btns">
            <button class="xbtn primary" data-report="${key}" data-fmt="csv">⬇ CSV</button>
            <button class="xbtn ghost" data-report="${key}" data-fmt="pdf">PDF</button>
            <button class="xbtn ghost" data-report="${key}" data-fmt="xlsx">Excel</button>
          </div>
        </div>`).join("")}
    </div>
    <p class="set-note">Every export pulls live data from the API — CSV, Excel (.xlsx) and PDF.</p>
    <div class="toast" id="toast"></div>
  `;

  const toast = root.querySelector("#toast");
  root.querySelectorAll("[data-fmt]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const r = REPORTS[btn.dataset.report];
      const fmt = btn.dataset.fmt;
      const label = btn.textContent;
      btn.textContent = "…";
      btn.disabled = true;
      try {
        const { headers, rows } = await r.build();
        const base = r.file.replace(/\.csv$/, "");
        if (fmt === "csv") downloadCsv(r.file, headers, rows);
        else if (fmt === "xlsx") downloadXlsx(`${base}.xlsx`, headers, rows, r.title.slice(0, 28));
        else if (fmt === "pdf") downloadPdf(`${base}.pdf`, r.title, headers, rows);
        showToast(toast, `Downloaded ${base}.${fmt}`);
      } catch (e) {
        showToast(toast, `Export failed: ${e.message}`);
      } finally {
        btn.textContent = label;
        btn.disabled = false;
      }
    });
  });
}

function showToast(el, msg) {
  el.textContent = msg;
  el.classList.add("show");
  clearTimeout(el._t);
  el._t = setTimeout(() => el.classList.remove("show"), 2600);
}
