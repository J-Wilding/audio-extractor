import { invoke } from "@tauri-apps/api/core";

let urlInputEl: HTMLInputElement | null;
let statusMsgEl: HTMLElement | null;

function extractAudio() {
  if (urlInputEl) {
    const url = urlInputEl.value;
    console.log("Extracting audio for URL:", url);
    if (statusMsgEl) {
      statusMsgEl.textContent = `Starting extraction for: ${url}`;
    }
  }
}

window.addEventListener("DOMContentLoaded", () => {
  urlInputEl = document.querySelector("#url-input");
  statusMsgEl = document.querySelector("#status-msg");
  
  document.querySelector("#extract-form")?.addEventListener("submit", (e) => {
    e.preventDefault();
    extractAudio();
  });
});
