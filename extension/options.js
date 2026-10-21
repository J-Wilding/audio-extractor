document.addEventListener("DOMContentLoaded", () => {
  const portEl = document.getElementById("port");
  const tokenEl = document.getElementById("token");
  const saveBtn = document.getElementById("save-btn");
  const statusEl = document.getElementById("status");

  chrome.storage.local.get(["bridgePort", "bridgeToken"], (data) => {
    if (data.bridgePort) portEl.value = data.bridgePort;
    if (data.bridgeToken) tokenEl.value = data.bridgeToken;
  });

  saveBtn.addEventListener("click", () => {
    const port = parseInt(portEl.value, 10) || 47821;
    const token = tokenEl.value.trim();

    chrome.storage.local.set({ bridgePort: port, bridgeToken: token }, () => {
      statusEl.style.color = "#2e7d32";
      statusEl.textContent = "✅ Saved successfully!";
      setTimeout(() => {
        statusEl.textContent = "";
      }, 2500);
    });
  });
});
