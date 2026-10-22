import {
  getHealth,
  getJobs,
  getSettings,
  normalizeServerUrl,
  notifyBadgeRefresh,
  saveSettings,
} from "./api.js";

document.addEventListener("DOMContentLoaded", async () => {
  const serverUrlEl = document.getElementById("server-url");
  const tokenEl = document.getElementById("token");
  const testBtn = document.getElementById("test-btn");
  const saveBtn = document.getElementById("save-btn");
  const statusEl = document.getElementById("status");

  function setStatus(msg, kind = "info") {
    statusEl.className = kind;
    statusEl.textContent = msg;
  }

  const current = await getSettings();
  serverUrlEl.value = current.serverUrl;
  tokenEl.value = current.token;

  async function ensureOriginPermission(serverUrl) {
    try {
      const u = new URL(serverUrl);
      const originPattern = `${u.protocol}//${u.hostname}/*`;
      const already = await chrome.permissions.contains({ origins: [originPattern] });
      if (!already) {
        await chrome.permissions.request({ origins: [originPattern] });
      }
    } catch {
      // Ignore if URL is malformed; validation will report it
    }
  }

  testBtn.addEventListener("click", async () => {
    const serverUrl = normalizeServerUrl(serverUrlEl.value);
    const token = tokenEl.value.trim();
    serverUrlEl.value = serverUrl;

    await ensureOriginPermission(serverUrl);
    setStatus("Testing connection…", "info");

    try {
      const health = await getHealth({ serverUrl, token });
      if (!token) {
        setStatus(
          `Server reachable (v${health.version}), but Bridge Token is empty. Enter your token to authenticate.`,
          "err"
        );
        return;
      }
      // Verify token against an authenticated endpoint (`/jobs` on v2 server, `/jobs/0` fallback on legacy bridge)
      try {
        await getJobs({ serverUrl, token });
      } catch (e) {
        if (e.code === "UNAUTHORIZED") throw e;
        // A 404 on /jobs means talking to legacy v1 desktop bridge with valid token
        if (e.status !== 404) throw e;
      }
      const engineInfo = health.engine_ready
        ? `yt-dlp ready${health.engine_version ? ` (${health.engine_version})` : ""}`
        : "yt-dlp updating…";
      setStatus(`✅ Connected to ${serverUrl} — v${health.version}, ${engineInfo}`, "ok");
    } catch (err) {
      setStatus(`❌ ${err.message}`, "err");
    }
  });

  saveBtn.addEventListener("click", async () => {
    const serverUrl = normalizeServerUrl(serverUrlEl.value);
    const token = tokenEl.value.trim();
    serverUrlEl.value = serverUrl;

    await ensureOriginPermission(serverUrl);
    await saveSettings({ serverUrl, token });
    notifyBadgeRefresh();
    setStatus(`✅ Saved settings (${serverUrl})`, "ok");
  });
});
