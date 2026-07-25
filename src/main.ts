import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

let urlInputEl: HTMLInputElement | null;
let statusMsgEl: HTMLElement | null;
let extractBtnEl: HTMLButtonElement | null;

async function extractAudio() {
  if (urlInputEl && statusMsgEl) {
    const url = urlInputEl.value;
    console.log("Extracting audio for URL:", url);
    
    statusMsgEl.innerHTML = `<span class="spinner"></span> Downloading...`;
    if (extractBtnEl) extractBtnEl.disabled = true;

    try {
      const result = await invoke<string>("start_extraction", { url });
      statusMsgEl.innerHTML = `✅ ${result}`;
      urlInputEl.value = "";
    } catch (e) {
      console.error(e);
      statusMsgEl.innerHTML = `❌ Error: ${e}`;
    } finally {
      if (extractBtnEl) extractBtnEl.disabled = false;
    }
  }
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

  document.querySelector("#extract-form")?.addEventListener("submit", (e) => {
    e.preventDefault();
    extractAudio();
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
});
