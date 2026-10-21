document.addEventListener("DOMContentLoaded", async () => {
  const optionsBtn = document.getElementById("options-btn");
  const typeBadge = document.getElementById("type-badge");
  const targetUrlEl = document.getElementById("target-url");
  const choiceContainer = document.getElementById("choice-container");
  const extractBtn = document.getElementById("extract-btn");
  const statusArea = document.getElementById("status-area");
  const progressContainer = document.getElementById("progress-container");
  const progressFill = document.getElementById("progress-fill");

  optionsBtn.addEventListener("click", (e) => {
    e.preventDefault();
    if (chrome.runtime.openOptionsPage) {
      chrome.runtime.openOptionsPage();
    } else {
      window.open(chrome.runtime.getURL("options.html"));
    }
  });

  function setStatus(msg, type = "info") {
    statusArea.className = "";
    if (type) statusArea.classList.add(type);
    statusArea.textContent = msg;
  }

  // Load port & token
  const config = await new Promise((resolve) => {
    chrome.storage.local.get(["bridgePort", "bridgeToken"], (data) => {
      resolve({
        port: data.bridgePort || 47821,
        token: data.bridgeToken || "",
      });
    });
  });

  const bridgeBase = `http://127.0.0.1:${config.port}`;

  if (!config.token) {
    setStatus("⚠️ Bridge token not configured. Click 'Settings ⚙️' to paste it from the app.", "error");
    extractBtn.disabled = true;
    return;
  }

  // Query active tab URL
  const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tabs || !tabs[0] || !tabs[0].url) {
    setStatus("Unable to detect active tab URL.", "error");
    targetUrlEl.textContent = "Unknown";
    return;
  }

  const rawUrl = tabs[0].url;
  targetUrlEl.textContent = rawUrl;

  let parsed;
  try {
    parsed = new URL(rawUrl);
  } catch (e) {
    typeBadge.textContent = "Invalid URL";
    typeBadge.className = "badge invalid";
    setStatus("Not a valid URL.", "error");
    return;
  }

  const host = parsed.hostname.toLowerCase();
  const allowedHosts = [
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "music.youtube.com",
    "youtu.be",
  ];

  if (!allowedHosts.includes(host)) {
    typeBadge.textContent = "Not YouTube";
    typeBadge.className = "badge invalid";
    setStatus("Active tab is not a YouTube or YouTube Music page.", "error");
    return;
  }

  // Classify URL
  const listParam = parsed.searchParams.get("list");
  const videoParam = parsed.searchParams.get("v") || (host === "youtu.be" ? parsed.pathname.slice(1) : null);
  const pathname = parsed.pathname;

  let classification = "unknown";
  let effectiveUrl = rawUrl;

  if (videoParam && listParam) {
    // Both song and list present (e.g. watch?v=...&list=RD...)
    classification = "choice";
    typeBadge.textContent = "Song in Playlist";
    typeBadge.className = "badge playlist";
    choiceContainer.style.display = "flex";
  } else if (listParam) {
    if (listParam.startsWith("OLAK5uy_") || pathname.includes("/browse/MPREb")) {
      classification = "album";
      typeBadge.textContent = "Album";
      typeBadge.className = "badge album";
    } else {
      classification = "playlist";
      typeBadge.textContent = "Playlist";
      typeBadge.className = "badge playlist";
    }
  } else if (videoParam || pathname.includes("/watch")) {
    classification = "song";
    typeBadge.textContent = "Song / Video";
    typeBadge.className = "badge song";
  } else {
    typeBadge.textContent = "YouTube Page";
    typeBadge.className = "badge invalid";
    setStatus("Current YouTube page doesn't appear to be a song, album, or playlist.", "error");
    return;
  }

  // Health check to ensure desktop app is open and ready
  try {
    const healthRes = await fetch(`${bridgeBase}/health`, {
      headers: { "x-bridge-token": config.token },
    });

    if (healthRes.status === 401) {
      setStatus("❌ Token rejected (401). Check token in Settings.", "error");
      return;
    }

    if (!healthRes.ok) {
      setStatus(`❌ Server error: HTTP ${healthRes.status}`, "error");
      return;
    }

    const health = await healthRes.json();
    if (!health.engine_ready) {
      setStatus("⏳ Audio Extractor engine is updating... Please wait.", "info");
      extractBtn.disabled = true;
      return;
    }
  } catch (err) {
    setStatus("⚠️ Audio Extractor app is not running. Please open the app first.", "error");
    return;
  }

  extractBtn.disabled = false;
  setStatus("Ready to extract.", "info");

  extractBtn.addEventListener("click", async () => {
    extractBtn.disabled = true;

    if (classification === "choice") {
      const choice = document.querySelector('input[name="url_choice"]:checked')?.value;
      if (choice === "song") {
        // Strip playlist parameter
        const u = new URL(rawUrl);
        u.searchParams.delete("list");
        u.searchParams.delete("index");
        effectiveUrl = u.toString();
      } else {
        effectiveUrl = rawUrl;
      }
    }

    setStatus("Submitting extraction job...", "info");
    progressContainer.style.display = "block";
    progressFill.style.width = "0%";

    let jobId = null;
    try {
      const submitRes = await fetch(`${bridgeBase}/extract`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "x-bridge-token": config.token,
        },
        body: JSON.stringify({ url: effectiveUrl }),
      });

      if (!submitRes.ok) {
        const errJson = await submitRes.json().catch(() => ({}));
        throw new Error(errJson.error || `HTTP ${submitRes.status}`);
      }

      const submitData = await submitRes.json();
      jobId = submitData.job_id;
      setStatus(`Job #${jobId} queued...`, "info");
    } catch (err) {
      setStatus(`Failed to submit: ${err.message}`, "error");
      extractBtn.disabled = false;
      return;
    }

    // Poll status while popup is open
    const pollInterval = setInterval(async () => {
      try {
        const statusRes = await fetch(`${bridgeBase}/jobs/${jobId}`, {
          headers: { "x-bridge-token": config.token },
        });

        if (!statusRes.ok) return;

        const job = await statusRes.json();

        if (job.progress) {
          const matchPercent = job.progress.match(/\[download\]\s+(\d+\.?\d*)%/);
          if (matchPercent) {
            progressFill.style.width = `${matchPercent[1]}%`;
          }
          setStatus(job.progress, "info");
        }

        if (job.status === "succeeded") {
          clearInterval(pollInterval);
          progressFill.style.width = "100%";
          setStatus(`✅ Completed! ${job.message || ""}`, "success");
        } else if (job.status === "failed") {
          clearInterval(pollInterval);
          setStatus(`❌ Failed: ${job.message || "Unknown error"}`, "error");
          extractBtn.disabled = false;
        }
      } catch (e) {
        // connection error during polling
      }
    }, 500);
  });
});
