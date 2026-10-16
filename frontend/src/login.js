import logoUrl from "./assets/sankayi-orange.png";
import { api } from "./api.js";
import { setToken } from "./auth.js";
import { esc } from "./format.js";

// Renders the full-page auth screen into #app. Calls onSuccess(user) after login.
export function renderLogin(onSuccess) {
  const app = document.getElementById("app");
  let mode = "login"; // "login" | "forgot"

  app.innerHTML = `
    <div class="auth-page">
      <div class="auth-photo"></div>
      <div class="auth-card">
        <div class="auth-brand">
          <div class="logo-disc" style="background-image:url('${logoUrl}')" role="img" aria-label="Sankayi"></div>
          <div class="mark">SANKAYI<span class="intel">INTELLIGENCE</span></div>
        </div>
        <div id="auth-view"></div>
      </div>
      <div class="auth-foot">Sankayi Jazz Series · internal analytics</div>
    </div>`;

  const view = app.querySelector("#auth-view");

  function paint() {
    view.innerHTML = mode === "login" ? loginForm() : forgotForm();
    mode === "login" ? wireLogin() : wireForgot();
  }

  function setError(msg) {
    const el = view.querySelector(".auth-error");
    if (el) { el.textContent = msg || ""; el.style.display = msg ? "block" : "none"; }
  }
  function setBusy(busy, btnLabel) {
    const btn = view.querySelector("button[type=submit]");
    if (btn) { btn.disabled = busy; btn.textContent = busy ? "…" : btnLabel; }
  }

  function wireLogin() {
    const form = view.querySelector("form");
    view.querySelector("[data-goto=forgot]").addEventListener("click", (e) => { e.preventDefault(); mode = "forgot"; paint(); });
    form.addEventListener("submit", async (e) => {
      e.preventDefault();
      setError("");
      const username = form.username.value.trim();
      const password = form.password.value;
      if (!username || !password) return setError("Enter your username and password.");
      setBusy(true, "Sign in");
      try {
        const res = await api.login(username, password);
        setToken(res.token);
        onSuccess(res.user);
      } catch (err) {
        setError(err.message || "Sign-in failed.");
        setBusy(false, "Sign in");
      }
    });
  }

  function wireForgot() {
    const form = view.querySelector("form");
    view.querySelector("[data-goto=login]").addEventListener("click", (e) => { e.preventDefault(); mode = "login"; paint(); });
    form.addEventListener("submit", async (e) => {
      e.preventDefault();
      setError("");
      const username = form.username.value.trim();
      const pw = form.newpw.value;
      const confirm = form.confirmpw.value;
      if (!username) return setError("Enter your username.");
      if (pw.length < 8) return setError("New password must be at least 8 characters.");
      if (pw !== confirm) return setError("Passwords don't match.");
      setBusy(true, "Reset password");
      try {
        await api.forgotPassword(username, pw);
        mode = "login";
        paint();
        const note = view.querySelector(".auth-note");
        if (note) { note.textContent = "Password reset — sign in with your new password."; note.style.display = "block"; }
      } catch (err) {
        setError(err.message || "Reset failed.");
        setBusy(false, "Reset password");
      }
    });
  }

  paint();
}

function loginForm() {
  return `
    <h1 class="auth-title">Sign in</h1>
    <div class="auth-note" style="display:none"></div>
    <form autocomplete="on" novalidate>
      <label class="auth-field"><span>Username</span>
        <input name="username" type="text" autocomplete="username" autofocus /></label>
      <label class="auth-field"><span>Password</span>
        <input name="password" type="password" autocomplete="current-password" /></label>
      <div class="auth-error" style="display:none"></div>
      <button type="submit" class="auth-btn">Sign in</button>
    </form>
    <a href="#" class="auth-link" data-goto="forgot">Forgot password?</a>`;
}

function forgotForm() {
  return `
    <h1 class="auth-title">Reset password</h1>
    <p class="auth-sub">Enter your username and a new password.</p>
    <form autocomplete="off" novalidate>
      <label class="auth-field"><span>Username</span>
        <input name="username" type="text" autofocus /></label>
      <label class="auth-field"><span>New password</span>
        <input name="newpw" type="password" autocomplete="new-password" /></label>
      <label class="auth-field"><span>Confirm new password</span>
        <input name="confirmpw" type="password" autocomplete="new-password" /></label>
      <div class="auth-error" style="display:none"></div>
      <button type="submit" class="auth-btn">Reset password</button>
    </form>
    <a href="#" class="auth-link" data-goto="login">← Back to sign in</a>`;
}
