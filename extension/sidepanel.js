import {
  cancelJob,
  clearJobs,
  classifyYouTubeUrl,
  getJobs,
  getLog,
  getSettings,
  openPopoutWindow,
  resolveSubmission,
  scanPlex,
  setLastSelectedJobId,
  submitJob,
} from "./api.js";

document.addEventListener("DOMContentLoaded", async () => {
  const connDot = document.getElementById("conn-dot");
  const plexScanBtn = document.getElementById("plex-scan-btn");
  const popoutBtn = document.getElementById("popout-btn");
  const settingsBtn = document.getElementById("settings-btn");
  const clearQueueBtn = document.getElementById("clear-queue-btn");
  const urlInput = document.getElementById("url-input");
  const modeSelect = document.getElementById("mode-select");
  const submitBtn = document.getElementById("submit-btn");
  const grabTabBtn = document.getElementById("grab-tab-btn");
  const banner = document.getElementById("banner");
  const jobsListEl = document.getElementById("jobs-list");
  const logTitleEl = document.getElementById("log-title");
  const logOutputEl = document.getElementById("log-output");
  const followBtn = document.getElementById("follow-btn");
  const copyLogBtn = document.getElementById("copy-log-btn");

  const params = new URLSearchParams(window.location.search);
  if (params.get("popout") === "1") {
    popoutBtn.style.display = "none";
  }

  let selectedJobId = null;
  let logCursor = 0;
  let autoScroll = true;
  let jobsCache = [];

  function showBanner(msg, kind = "info", timeoutMs = 4000) {
    banner.className = kind;
    banner.textContent = msg;
    if (timeoutMs > 0) {
      setTimeout(() => {
        if (banner.textContent === msg) {
          banner.className = "";
          banner.textContent = "";
        }
      }, timeoutMs);
    }
  }

  settingsBtn.addEventListener("click", () => {
    if (chrome.runtime.openOptionsPage) {
      chrome.runtime.openOptionsPage();
    } else {
      window.open(chrome.runtime.getURL("options.html"));
    }
  });

  popoutBtn.addEventListener("click", async () => {
    await openPopoutWindow(selectedJobId);
  });

  clearQueueBtn.addEventListener("click", async () => {
    try {
      const res = await clearJobs();
      showBanner(`Cleared ${res.cleared || 0} job(s).`, "info", 2000);
      await pollJobs();
      if (!jobsCache.some((j) => j.id === selectedJobId)) {
        selectedJobId = null;
        logOutputEl.textContent = "";
        logTitleEl.textContent = "Select a job to view live output";
      }
    } catch (err) {
      showBanner(`❌ Failed to clear queue: ${err.message}`, "error");
    }
  });

  plexScanBtn.addEventListener("click", async () => {
    plexScanBtn.disabled = true;
    try {
      const res = await scanPlex();
      showBanner(`✅ ${res.message || "Plex scan triggered."}`, "success");
    } catch (err) {
      showBanner(`❌ Plex scan failed: ${err.message}`, "error");
    } finally {
      plexScanBtn.disabled = false;
    }
  });

  logOutputEl.addEventListener("scroll", () => {
    const distanceFromBottom =
      logOutputEl.scrollHeight - logOutputEl.scrollTop - logOutputEl.clientHeight;
    autoScroll = distanceFromBottom < 24;
    followBtn.style.display = autoScroll ? "none" : "inline-block";
  });

  followBtn.addEventListener("click", () => {
    autoScroll = true;
    followBtn.style.display = "none";
    logOutputEl.scrollTop = logOutputEl.scrollHeight;
  });

  copyLogBtn.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(logOutputEl.textContent || "");
      showBanner("✅ Copied log to clipboard.", "success", 2000);
    } catch {
      showBanner("Failed to copy log.", "error");
    }
  });

  async function selectJob(jobId) {
    if (selectedJobId === jobId) return;
    selectedJobId = jobId;
    logCursor = 0;
    autoScroll = true;
    followBtn.style.display = "none";
    logOutputEl.textContent = "";
    await setLastSelectedJobId(jobId);
    renderJobList(jobsCache);
    await pollSelectedLog();
  }

  function formatSubline(job) {
    const parts = [];
    if (job.progress && job.progress.item && job.progress.total) {
      parts.push(`Track ${job.progress.item}/${job.progress.total}`);
    } else if (job.tracks_done > 0) {
      parts.push(`${job.tracks_done} track${job.tracks_done === 1 ? "" : "s"}`);
    }
    if (job.status === "running" && job.progress && typeof job.progress.percent === "number") {
      parts.push(`${job.progress.percent.toFixed(0)}%`);
    }
    if (job.message && (job.status === "succeeded" || job.status === "failed" || job.status === "cancelled")) {
      parts.push(job.message);
    } else if (job.last_line) {
      parts.push(job.last_line);
    } else {
      parts.push(job.url);
    }
    return parts.join(" · ");
  }

  function renderJobList(jobs) {
    jobsCache = jobs;
    if (!jobs || jobs.length === 0) {
      jobsListEl.innerHTML = `<div class="empty-state">No jobs yet. Send a YouTube or YouTube Music link above!</div>`;
      return;
    }

    jobsListEl.innerHTML = "";
    for (const job of jobs) {
      const card = document.createElement("div");
      card.className = `job-card${job.id === selectedJobId ? " selected" : ""}`;

      const top = document.createElement("div");
      top.className = "job-top";

      const title = document.createElement("div");
      title.className = "job-title";
      title.textContent = `#${job.id} ${job.title || job.url}`;
      top.appendChild(title);

      if (job.status === "queued" || job.status === "running") {
        const cancelBtn = document.createElement("button");
        cancelBtn.className = "cancel-btn";
        cancelBtn.textContent = "Cancel";
        cancelBtn.addEventListener("click", async (e) => {
          e.stopPropagation();
          cancelBtn.disabled = true;
          try {
            await cancelJob(job.id);
            await pollJobs();
          } catch (err) {
            showBanner(`❌ ${err.message}`, "error");
          }
        });
        top.appendChild(cancelBtn);
      }

      const pill = document.createElement("span");
      pill.className = `status-pill ${job.status}`;
      pill.textContent = job.status;
      top.appendChild(pill);

      const sub = document.createElement("div");
      sub.className = "job-sub";
      sub.textContent = formatSubline(job);

      card.appendChild(top);
      card.appendChild(sub);

      if (job.status === "running") {
        const bar = document.createElement("div");
        bar.className = "job-bar";
        const fill = document.createElement("div");
        fill.className = "job-bar-fill";
        const pct =
          job.progress && typeof job.progress.percent === "number"
            ? Math.max(2, job.progress.percent)
            : 5;
        fill.style.width = `${pct}%`;
        bar.appendChild(fill);
        card.appendChild(bar);
      }

      card.addEventListener("click", () => selectJob(job.id));
      jobsListEl.appendChild(card);
    }
  }

  async function pollJobs() {
    if (document.hidden) return;
    try {
      const jobs = await getJobs();
      connDot.className = "conn-dot online";

      if (!selectedJobId && jobs.length > 0) {
        const settings = await getSettings();
        const preferred =
          jobs.find((j) => j.id === settings.lastSelectedJobId) ||
          jobs.find((j) => j.status === "running" || j.status === "queued") ||
          jobs[0];
        selectedJobId = preferred.id;
        logCursor = 0;
        logOutputEl.textContent = "";
      }

      renderJobList(jobs);
    } catch (err) {
      connDot.className = "conn-dot offline";
      showBanner(`⚠️ ${err.message}`, "error", 0);
    }
  }

  async function pollSelectedLog() {
    if (document.hidden || !selectedJobId) return;
    const job = jobsCache.find((j) => j.id === selectedJobId);
    if (job) {
      logTitleEl.textContent = `Log — Job #${job.id} (${job.title || job.status})`;
    } else {
      logTitleEl.textContent = `Log — Job #${selectedJobId}`;
    }

    try {
      const page = await getLog(selectedJobId, logCursor);
      if (page.truncated && logCursor === 0) {
        logOutputEl.textContent += "… (earlier lines truncated) …\n";
      }
      if (page.lines && page.lines.length > 0) {
        const chunk = page.lines.map((l) => l.line).join("\n") + "\n";
        logOutputEl.textContent += chunk;
        logCursor = page.next;
        if (autoScroll) {
          logOutputEl.scrollTop = logOutputEl.scrollHeight;
        }
      }
    } catch {
      // ignore transient errors
    }
  }

  async function queueUrl(rawUrl, explicitMode = null) {
    const classification = classifyYouTubeUrl(rawUrl);
    if (!classification.valid) {
      showBanner(`❌ ${classification.reason}`, "error");
      return;
    }
    const mode = explicitMode || classification.defaultMode || "playlist";
    const { url, mode: resolvedMode } = resolveSubmission(rawUrl, classification, mode);
    try {
      const job = await submitJob(url, resolvedMode);
      const id = job.id || job.job_id;
      urlInput.value = "";
      showBanner(`✅ Queued Job #${id}`, "success", 2500);
      await pollJobs();
      await selectJob(id);
    } catch (err) {
      showBanner(`❌ ${err.message}`, "error");
    }
  }

  submitBtn.addEventListener("click", async () => {
    await queueUrl(urlInput.value, modeSelect.value);
  });

  urlInput.addEventListener("keydown", async (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      await queueUrl(urlInput.value, modeSelect.value);
    }
  });

  grabTabBtn.addEventListener("click", async () => {
    const tabs = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
    const tab = (tabs || []).find((t) => t.url && t.url.startsWith("http"));
    if (!tab || !tab.url) {
      showBanner("❌ No active YouTube tab found.", "error");
      return;
    }
    await queueUrl(tab.url, null);
  });

  // Listen for storage changes so clicking "Extract" in the popup switches the Side Panel to the new job immediately
  chrome.storage.onChanged.addListener((changes, area) => {
    if (area === "local" && changes.lastSelectedJobId && changes.lastSelectedJobId.newValue) {
      const newId = changes.lastSelectedJobId.newValue;
      pollJobs().then(() => selectJob(newId));
    }
  });

  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) {
      pollJobs();
      pollSelectedLog();
    }
  });

  await pollJobs();
  await pollSelectedLog();
  setInterval(pollJobs, 2000);
  setInterval(pollSelectedLog, 1000);
});
