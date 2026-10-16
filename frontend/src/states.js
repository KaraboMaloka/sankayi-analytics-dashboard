import { esc } from "./format.js";
import { icons } from "./icons.js";

export function loading(root, msg = "Loading…") {
  root.innerHTML = `<div class="state"><div class="spinner"></div><p>${esc(msg)}</p></div>`;
}

export function errorState(root, err) {
  const isNetwork = err?.code === "network";
  root.innerHTML = `
    <div class="state error">
      <h3>${isNetwork ? "Can't reach the API" : "Something went wrong"}</h3>
      <p>${esc(err?.message || "Unexpected error.")}</p>
      ${isNetwork ? `<p style="font-size:12px">Start it with <code>cargo run</code> in the <code>backend</code> folder, then reload.</p>` : ""}
    </div>`;
}

export function placeholderPage(root, title, blurb) {
  root.innerHTML = `
    <div class="panel">
      <div class="placeholder-hero">
        <div class="pico">${icons.forecasts}</div>
        <div class="state">
          <h3>${esc(title)} — coming in a later phase</h3>
          <p>${esc(blurb)}</p>
        </div>
      </div>
    </div>`;
}
