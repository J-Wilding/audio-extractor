import {
  classifyYouTubeUrl,
  getHealth,
  getJob,
  getJobs,
  getSettings,
  openPopoutWindow,
  resolveSubmission,
  setLastSelectedJobId,
  submitJob,
} from "./api.js";

document.addEventListener("DOMContentLoaded", async () => {
  const optionsBtn = document.getElementById("options-btn");
  const typeBadge = document.getElementById("type-badge");
  const targetUrlEl = document.getElementById("target-url");
  const choiceContainer = document.getElementById("choice-container");
  const extractBtn = document.getElementById("extract-btn");
  const statusArea = document.getElementById("status-area");
  const progressContainer = document.getElementById("progress-container");
  const progressFill = document.getElementById("progress-fill");
  const openSidePanelBtn = document.getElementById("open-sidepanel-btn");
  const openPopoutBtn = document.getElementById("open-popout-btn");

  let activePollInterval = null;
  let watchedJobId = null;

  function setStatus(msg, type = "info") {
    statusArea.className = "";
    if (type) statusArea.classList.add(type);
    statusArea.textContent = msg;
  }

  optionsBtn.addEventListener("click", (e) => {
    e.preventDefault();
    if (chrome.runtime.openOptionsPage) {
      chrome.runtime.openOptionsPage();
    } else {
      window.open(chrome.runtime.getURL("options.html"));
    }
  });

  openSidePanelBtn.addEventListener("click", async () => {
    try {
      if (watchedJobId) {
        await setLastSelectedJobId(watchedJobId);
      }
      const win = await chrome.windows.getCurrent();
      await chrome.sidePanel.open({ windowId: win.id });
      window.close();
    } catch {
      await openPopoutWindow(watchedJobId);
      window.close();
    }
  });

  openPopoutBtn.addEventListener("click", async () => {
    await openPopoutWindow(watchedJobId);
    window.close();
  });

  function startWatchingJob(jobId) {
    watchedJobId = jobId;
    if (activePollInterval) clearInterval(activePollInterval);
    progressContainer.style.display = "block";

    const poll = async () => {
      try {
        const job = await getJob(jobId);
        const pct = job.progress && typeof job.progress.percent === "number" ? job.progress.percent : null;
        if (pct !== null) {
          progressFill.style.width = `${pct}%`;
        }
        const itemStr =
          job.progress && job.progress.item && job.progress.total
            ? `[Track ${job.progress.item}/${job.progress.total}] `
            : "";
        const line = job.last_line || job.progress || "";

        if (job.status === "succeeded") {
          clearInterval(activePollInterval);
          progressFill.style.width = "100%";
          setStatus(`✅ Job #${job.id}: ${job.message || "Completed!"}`, "success");
        } else if (job.status === "failed") {
          clearInterval(activePollInterval);
          setStatus(`❌ Job #${job.id} failed: ${job.message || "Unknown error"}`, "error");
        } else if (job.status === "cancelled") {
          clearInterval(activePollInterval);
          setStatus(`⏹ Job #${job.id} cancelled.`, "info");
        } else {
          setStatus(
            `⏳ Job #${job.id} (${job.status}): ${itemStr}${typeof line === "string" ? line : "Working…"}`,
            "info"
          );
        }
      } catch {
        // ignore transient poll errors while popup is open
      }
    };

    poll();
    activePollInterval = setInterval(poll, 750);
  }

  const settings = await getSettings();
  if (!settings.token) {
    setStatus("⚠️ Token not configured. Click 'Settings ⚙️' to set your server URL and token.", "error");
    return;
  }

  // Inspect active tab
  const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
  const rawUrl = tabs && tabs[0] && tabs[0].url ? tabs[0].url : "";
  targetUrlEl.textContent = rawUrl || "Unknown";

  const classification = classifyYouTubeUrl(rawUrl);
  typeBadge.textContent = classification.label;
  typeBadge.className = `badge ${classification.badgeClass}`;
  if (classification.hasChoice) {
    choiceContainer.style.display = "flex";
  }

  // Check server health & whether a job is already running (catch-up when reopening popup!)
  try {
    const health = await getHealth(settings);
    if (!health.engine_ready) {
      setStatus("⏳ Server is updating yt-dlp… You can still queue jobs.", "info");
    }

    // Check if there is an active job right now so reopening the popup reconnects immediately
    try {
      const jobs = await getJobs(settings);
      const activeJob = jobs.find((j) => j.status === "running" || j.status === "queued");
      if (activeJob) {
        startWatchingJob(activeJob.id);
      } else if (classification.valid) {
        setStatus(`Ready to extract (${settings.serverUrl}).`, "info");
      } else {
        setStatus(classification.reason, "info");
      }
    } catch {
      if (classification.valid) {
        setStatus(`Ready to extract (${settings.serverUrl}).`, "info");
      } else {
        setStatus(classification.reason, "error");
      }
    }
  } catch (err) {
    setStatus(`⚠️ ${err.message}`, "error");
    return;
  }

  if (classification.valid) {
    extractBtn.disabled = false;
  }

  extractBtn.addEventListener("click", async () => {
    extractBtn.disabled = true;
    const chosenMode = document.querySelector('input[name="url_choice"]:checked')?.value || "song";
    const { url, mode } = resolveSubmission(rawUrl, classification, chosenMode);

    setStatus("Submitting job…", "info");
    try {
      const submitted = await submitJob(url, mode, settings);
      const jobId = submitted.id || submitted.job_id;
      await setLastSelectedJobId(jobId);
      setStatus(`✅ Queued Job #${jobId}. Open Side Panel or Pop-out to keep watching.`, "info");
      startWatchingJob(jobId);
    } catch (err) {
      setStatus(`❌ Failed to submit: ${err.message}`, "error");
      extractBtn.disabled = false;
    }
  });
});
