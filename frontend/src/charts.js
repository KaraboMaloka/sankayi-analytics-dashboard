// Hand-built SVG area chart with a crosshair tooltip. Single gold series.
// points: [{ label: "20 Jun", value: 60, tip?: "..." }]

import { num } from "./format.js";

export function areaChartHTML(id) {
  return `
    <div class="chart-wrap" data-chart="${id}">
      <svg class="area" viewBox="0 0 640 250" role="img" aria-label="Cumulative series">
        <defs>
          <linearGradient id="fill-${id}" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color="rgba(236,106,44,0.38)"/>
            <stop offset="100%" stop-color="rgba(236,106,44,0.02)"/>
          </linearGradient>
        </defs>
        <g class="grid"></g>
        <path class="area-fill" fill="url(#fill-${id})"></path>
        <path class="area-line"></path>
        <line class="hover-line" y1="14" y2="220"></line>
        <circle class="end-dot" r="5"></circle>
        <circle class="hover-dot" r="5.5"></circle>
        <g class="xlabels"></g>
      </svg>
      <div class="tooltip"><div class="t-val"></div><div class="t-date"></div></div>
    </div>`;
}

export function drawAreaChart(root, id, points, opts = {}) {
  const wrap = root.querySelector(`[data-chart="${id}"]`);
  if (!wrap || points.length === 0) return;
  const svg = wrap.querySelector("svg");
  const NS = "http://www.w3.org/2000/svg";
  const W = 640, top = 14, baseY = 220, left = 42, right = 622;
  const maxV = opts.max ?? (Math.max(...points.map((p) => p.value)) * 1.05 || 1);
  const x = (i) => left + (i * (right - left)) / Math.max(points.length - 1, 1);
  const y = (v) => baseY - (v / maxV) * (baseY - top);

  // gridlines
  const grid = svg.querySelector(".grid");
  const ticks = opts.ticks ?? niceTicks(maxV);
  ticks.forEach((v) => {
    const gy = y(v);
    const line = document.createElementNS(NS, "line");
    line.setAttribute("class", "grid-line");
    line.setAttribute("x1", left); line.setAttribute("x2", right);
    line.setAttribute("y1", gy); line.setAttribute("y2", gy);
    grid.appendChild(line);
    const t = document.createElementNS(NS, "text");
    t.setAttribute("class", "axis-label");
    t.setAttribute("x", left - 8); t.setAttribute("y", gy + 4); t.setAttribute("text-anchor", "end");
    t.textContent = num(v);
    grid.appendChild(t);
  });

  // paths
  let line = "";
  points.forEach((p, i) => { line += (i === 0 ? "M" : "L") + x(i).toFixed(1) + "," + y(p.value).toFixed(1) + " "; });
  const area = line + "L" + x(points.length - 1).toFixed(1) + "," + baseY + " L" + x(0).toFixed(1) + "," + baseY + " Z";
  svg.querySelector(".area-line").setAttribute("d", line.trim());
  svg.querySelector(".area-fill").setAttribute("d", area);

  const endDot = svg.querySelector(".end-dot");
  const last = points.length - 1;
  endDot.setAttribute("cx", x(last)); endDot.setAttribute("cy", y(points[last].value));

  // x labels (thin them out)
  const xl = svg.querySelector(".xlabels");
  const step = Math.ceil(points.length / 6);
  points.forEach((p, i) => {
    if (i % step !== 0 && i !== last) return;
    const t = document.createElementNS(NS, "text");
    t.setAttribute("class", "axis-label");
    t.setAttribute("x", x(i)); t.setAttribute("y", baseY + 20); t.setAttribute("text-anchor", "middle");
    t.textContent = p.label;
    xl.appendChild(t);
  });

  // hover
  const hLine = svg.querySelector(".hover-line");
  const hDot = svg.querySelector(".hover-dot");
  const tip = wrap.querySelector(".tooltip");
  const tVal = tip.querySelector(".t-val");
  const tDate = tip.querySelector(".t-date");
  const unit = opts.unit ?? "";

  svg.addEventListener("mousemove", (e) => {
    const rect = svg.getBoundingClientRect();
    const px = ((e.clientX - rect.left) / rect.width) * W;
    let idx = 0, best = Infinity;
    points.forEach((p, i) => { const d = Math.abs(x(i) - px); if (d < best) { best = d; idx = i; } });
    const gx = x(idx), gy = y(points[idx].value);
    hLine.setAttribute("x1", gx); hLine.setAttribute("x2", gx); hLine.style.opacity = 1;
    hDot.setAttribute("cx", gx); hDot.setAttribute("cy", gy); hDot.style.opacity = 1;
    tip.style.opacity = 1;
    tip.style.left = (gx / W) * rect.width + "px";
    tip.style.top = (gy / 250) * rect.height + "px";
    tVal.textContent = num(points[idx].value) + (unit ? " " + unit : "");
    tDate.textContent = points[idx].tip ?? points[idx].label;
  });
  svg.addEventListener("mouseleave", () => {
    hLine.style.opacity = 0; hDot.style.opacity = 0; tip.style.opacity = 0;
  });
}

function niceTicks(max) {
  const step = Math.pow(10, Math.floor(Math.log10(max)));
  const rounded = Math.ceil(max / step) * step;
  return [0, rounded * 0.33, rounded * 0.66, rounded].map((v) => Math.round(v / 10) * 10);
}
