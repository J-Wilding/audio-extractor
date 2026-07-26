import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

let urlInputEl: HTMLInputElement | null;
let statusMsgEl: HTMLElement | null;
let extractBtnEl: HTMLButtonElement | null;

async function extractAudio() {
  if (urlInputEl && statusMsgEl) {
    const url = urlInputEl.value;
    console.log("Extracting audio for URL:", url);

    statusMsgEl.innerHTML = `<span class="spinner"></span> Downloading...\n`;
    if (extractBtnEl) extractBtnEl.disabled = true;

    const progressContainer = document.getElementById("progress-container");
    const progressText = document.getElementById("progress-text");
    const progressPercent = document.getElementById("progress-percent");
    const progressFill = document.getElementById("progress-fill");

    if (progressContainer) progressContainer.style.display = "block";
    if (progressText) progressText.innerText = "Downloading...";
    if (progressPercent) progressPercent.innerText = "0%";
    if (progressFill) progressFill.style.width = "0%";

    const unlisten = await listen<string>("extraction-progress", (event) => {
      if (statusMsgEl) {
        statusMsgEl.insertAdjacentText('beforeend', event.payload + "\n");
        statusMsgEl.scrollTop = statusMsgEl.scrollHeight;

        const matchPercent = event.payload.match(/\[download\]\s+(\d+\.?\d*)%/);
        if (matchPercent && progressPercent && progressFill) {
          const percent = matchPercent[1];
          progressPercent.innerText = `${percent}%`;
          progressFill.style.width = `${percent}%`;
        }

        const matchItem = event.payload.match(/\[download\] Downloading (?:video|item) (\d+) of (\d+)/i);
        if (matchItem && progressText) {
          progressText.innerText = `Downloading Track ${matchItem[1]} of ${matchItem[2]}...`;
        }
      }
    });

    try {
      const result = await invoke<string>("start_extraction", { url });
      statusMsgEl.insertAdjacentHTML('beforeend', `✅ ${result}`);
      statusMsgEl.scrollTop = statusMsgEl.scrollHeight;
      urlInputEl.value = "";
    } catch (e) {
      console.error(e);
      statusMsgEl.insertAdjacentHTML('beforeend', `❌ Error: ${e}`);
      statusMsgEl.scrollTop = statusMsgEl.scrollHeight;
    } finally {
      unlisten();
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
