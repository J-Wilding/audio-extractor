import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

let urlInputEl: HTMLInputElement | null;
let statusMsgEl: HTMLElement | null;
let extractBtnEl: HTMLButtonElement | null;

/** Mirrors `bridge::Job` in src-tauri/src/bridge.rs. */
interface BridgeJob {
  id: number;
  url: string;
  status: "queued" | "running" | "succeeded" | "failed";
  progress: string | null;
  message: string | null;
}

/** Mirrors `BridgeInfo` in src-tauri/src/lib.rs. */
interface BridgeInfo {
  port: number;
  token: string | null;
  error: string | null;
}

interface AppConfig {
  plex_url: string;
  plex_section_id: string;
  plex_token: string | null;
  mac_music_root: string;
  plex_music_root: string;
  playlists_dir: string;
}

async function openSettingsModal() {
  const modal = document.getElementById("settings-modal");
  if (!modal) return;
  try {
    const cfg = await invoke<AppConfig>("get_app_config");
    (document.getElementById("cfg-plex-token") as HTMLInputElement).value = cfg.plex_token || "";
    (document.getElementById("cfg-plex-url") as HTMLInputElement).value = cfg.plex_url || "";
    (document.getElementById("cfg-section-id") as HTMLInputElement).value = cfg.plex_section_id || "";
    (document.getElementById("cfg-mac-music-root") as HTMLInputElement).value = cfg.mac_music_root || "";
    (document.getElementById("cfg-plex-music-root") as HTMLInputElement).value = cfg.plex_music_root || "";
    (document.getElementById("cfg-playlists-dir") as HTMLInputElement).value = cfg.playlists_dir || "";
    modal.style.display = "flex";
  } catch (err) {
    console.error("Failed to load settings:", err);
  }
}

function closeSettingsModal() {
  const modal = document.getElementById("settings-modal");
  if (modal) modal.style.display = "none";
}

async function saveSettingsFromModal() {
  const tokenVal = (document.getElementById("cfg-plex-token") as HTMLInputElement).value.trim();
  let plexUrl = (document.getElementById("cfg-plex-url") as HTMLInputElement).value.trim();
  if (plexUrl && !plexUrl.startsWith("http://") && !plexUrl.startsWith("https://")) {
    plexUrl = `http://${plexUrl}`;
  }

  const cfg: AppConfig = {
    plex_token: tokenVal.length > 0 ? tokenVal : null,
    plex_url: plexUrl,
    plex_section_id: (document.getElementById("cfg-section-id") as HTMLInputElement).value.trim(),
    mac_music_root: (document.getElementById("cfg-mac-music-root") as HTMLInputElement).value.trim(),
    plex_music_root: (document.getElementById("cfg-plex-music-root") as HTMLInputElement).value.trim(),
    playlists_dir: (document.getElementById("cfg-playlists-dir") as HTMLInputElement).value.trim(),
  };

  try {
    const res = await invoke<string>("save_app_config", { config: cfg });
    closeSettingsModal();
    if (statusMsgEl) statusMsgEl.innerHTML = `✅ ${res}`;
  } catch (err) {
    alert(`Failed to save settings: ${err}`);
  }
}

/** Resets the progress UI for a new job. Shared by GUI and Browser Bridge jobs. */
function startProgressUi(label: string) {
  if (statusMsgEl) statusMsgEl.innerHTML = `<span class="spinner"></span> ${label}\n`;
  if (extractBtnEl) extractBtnEl.disabled = true;

  const progressContainer = document.getElementById("progress-container");
  const progressText = document.getElementById("progress-text");
  const progressPercent = document.getElementById("progress-percent");
  const progressFill = document.getElementById("progress-fill");

  if (progressContainer) progressContainer.style.display = "block";
  if (progressText) progressText.innerText = "Downloading...";
  if (progressPercent) progressPercent.innerText = "0%";
  if (progressFill) progressFill.style.width = "0%";
}

/** Handles one Engine output line (from either entry point). */
function onProgressLine(line: string) {
  if (!statusMsgEl) return;
  const progressText = document.getElementById("progress-text");
  const progressPercent = document.getElementById("progress-percent");
  const progressFill = document.getElementById("progress-fill");

  statusMsgEl.insertAdjacentText('beforeend', line + "\n");
  statusMsgEl.scrollTop = statusMsgEl.scrollHeight;

  const matchPercent = line.match(/\[download\]\s+(\d+\.?\d*)%/);
  if (matchPercent && progressPercent && progressFill) {
    const percent = matchPercent[1];
    progressPercent.innerText = `${percent}%`;
    progressFill.style.width = `${percent}%`;
  }

  const matchItem = line.match(/\[download\] Downloading (?:video|item) (\d+) of (\d+)/i);
  if (matchItem && progressText) {
    progressText.innerText = `Downloading Track ${matchItem[1]} of ${matchItem[2]}...`;
  }
}

function finishProgressUi(ok: boolean, message: string) {
  if (statusMsgEl) {
    if (ok) {
      statusMsgEl.insertAdjacentHTML('beforeend', `✅ `);
      statusMsgEl.insertAdjacentText('beforeend', message);
    } else {
      statusMsgEl.insertAdjacentHTML('beforeend', `❌ `);
      statusMsgEl.insertAdjacentText('beforeend', `Error: ${message}`);
    }
    statusMsgEl.scrollTop = statusMsgEl.scrollHeight;
  }
  if (extractBtnEl) extractBtnEl.disabled = false;
}

async function extractAudio() {
  if (urlInputEl && statusMsgEl) {
    const url = urlInputEl.value;
    console.log("Extracting audio for URL:", url);

    startProgressUi("Downloading...");

    try {
      const result = await invoke<string>("start_extraction", { url });
      finishProgressUi(true, result);
      urlInputEl.value = "";
    } catch (e) {
      console.error(e);
      finishProgressUi(false, String(e));
    }
  }
}

/** Reflects jobs submitted by the browser extension in the main window. */
async function setupBridgeJobs() {
  await listen<BridgeJob>("bridge-job", (event) => {
    const job = event.payload;
    if (job.status === "running") {
      startProgressUi("Downloading (from browser)...");
      onProgressLine(job.url);
    } else if (job.status === "succeeded" || job.status === "failed") {
      finishProgressUi(job.status === "succeeded", job.message ?? "");
    }
  });
}

/** Shows the Bridge port/token and copies the token for the extension's options page. */
async function showBridgeInfo() {
  if (!statusMsgEl) return;
  const info = await invoke<BridgeInfo>("get_bridge_info");
  if (info.error || !info.token) {
    statusMsgEl.innerText = `❌ ${info.error ?? "Browser Bridge is not running."}`;
    return;
  }
  let copied = false;
  try {
    await navigator.clipboard.writeText(info.token);
    copied = true;
  } catch (e) {
    console.error("Clipboard write failed:", e);
  }
  statusMsgEl.innerText =
    `Browser Bridge: http://127.0.0.1:${info.port}\n` +
    `Token: ${info.token}\n` +
    (copied ? "✅ Token copied to clipboard. Paste it into the extension's options." : "");
}

async function setupEngineLock() {
  // Listen for real-time engine status updates
  await listen<string>("engine-status", (event) => {
    if (statusMsgEl) {
      if (event.payload === "Ready") {
        statusMsgEl.innerHTML = "";
        if (extractBtnEl) extractBtnEl.disabled = false;
      } else {
        statusMsgEl.innerHTML = `<span class="spinner"></span> ${event.payload}`;
        if (extractBtnEl) extractBtnEl.disabled = true;
      }
    }
  });

  // Check initial state on mount
  const isReady = await invoke<boolean>("get_engine_status");
  if (!isReady && statusMsgEl) {
    statusMsgEl.innerHTML = `<span class="spinner"></span> Updating Engine...`;
    if (extractBtnEl) extractBtnEl.disabled = true;
  }
}

window.addEventListener("DOMContentLoaded", () => {
  urlInputEl = document.querySelector("#url-input");
  statusMsgEl = document.querySelector("#status-msg");
  extractBtnEl = document.querySelector("button[type='submit']");

  setupEngineLock();
  setupBridgeJobs();
  // One global listener for Engine output, whichever entry point started the job.
  listen<string>("extraction-progress", (event) => onProgressLine(event.payload));

  document.querySelector("#extract-form")?.addEventListener("submit", (e) => {
    e.preventDefault();
    extractAudio();
  });

  document.querySelector("#bridge-btn")?.addEventListener("click", () => {
    showBridgeInfo();
  });

  document.querySelector("#plex-scan-btn")?.addEventListener("click", async () => {
    if (statusMsgEl) {
      statusMsgEl.innerHTML = `<span class="spinner"></span> Triggering Plex library scan...`;
    }
    try {
      const result = await invoke<string>("trigger_plex_scan");
      if (statusMsgEl) statusMsgEl.innerHTML = `✅ ${result}`;
    } catch (e) {
      if (statusMsgEl) statusMsgEl.innerHTML = `❌ Error scanning Plex: ${e}`;
    }
  });

  document.querySelector("#settings-btn")?.addEventListener("click", () => {
    openSettingsModal();
  });
  document.querySelector("#close-settings-btn")?.addEventListener("click", () => {
    closeSettingsModal();
  });
  document.querySelector("#cancel-settings-btn")?.addEventListener("click", () => {
    closeSettingsModal();
  });
  document.querySelector("#save-settings-btn")?.addEventListener("click", () => {
    saveSettingsFromModal();
  });

  document.querySelector("#update-btn")?.addEventListener("click", async () => {
    if (statusMsgEl) {
      statusMsgEl.innerHTML = `<span class="spinner"></span> Checking for updates...`;
    }
    try {
      const result = await invoke<string>("update_engine");
      if (statusMsgEl) statusMsgEl.innerHTML = `✅ ${result}`;
    } catch (e) {
      if (statusMsgEl) statusMsgEl.innerHTML = `❌ Error updating: ${e}`;
    }
  });

  const viewSizes = [
    { name: 'View: Large', width: 950, height: 950, scale: 1.0 },
    { name: 'View: Small', width: 665, height: 720, scale: 0.70 }
  ];
  let currentSizeIndex = 0;

  document.querySelector("#view-btn")?.addEventListener("click", async (e) => {
    currentSizeIndex = (currentSizeIndex + 1) % viewSizes.length;
    const newSize = viewSizes[currentSizeIndex];

    // Use CSS zoom on the body so EVERYTHING scales perfectly together (menu bar, padding, casing)
    (document.body.style as any).zoom = newSize.scale;

    // Resize the Tauri window so it tightly wraps the newly scaled content
    try {
      await invoke("resize_window", { width: newSize.width, height: newSize.height });
    } catch (err) {
      console.error("Failed to resize window:", err);
    }

    // Update button text
    (e.target as HTMLElement).innerText = newSize.name;
  });
});
