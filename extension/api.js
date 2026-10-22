export const DEFAULT_SERVER_URL = "http://10.0.0.69:47821";

const ALLOWED_HOSTS = [
  "youtube.com",
  "www.youtube.com",
  "m.youtube.com",
  "music.youtube.com",
  "youtu.be",
];

export function normalizeServerUrl(raw) {
  const trimmed = (raw || "").trim().replace(/\/+$/, "");
  if (!trimmed) return DEFAULT_SERVER_URL;
  if (!trimmed.startsWith("http://") && !trimmed.startsWith("https://")) {
    return `http://${trimmed}`;
  }
  return trimmed;
}

export async function getSettings() {
  return new Promise((resolve) => {
    chrome.storage.local.get(
      ["serverUrl", "bridgePort", "bridgeToken", "lastSelectedJobId"],
      (data) => {
        let serverUrl = data.serverUrl;
        if (!serverUrl) {
          serverUrl = data.bridgePort
            ? `http://127.0.0.1:${data.bridgePort}`
            : DEFAULT_SERVER_URL;
        }
        resolve({
          serverUrl: normalizeServerUrl(serverUrl),
          token: (data.bridgeToken || "").trim(),
          lastSelectedJobId: data.lastSelectedJobId || null,
        });
      }
    );
  });
}

export async function saveSettings({ serverUrl, token }) {
  const normalized = normalizeServerUrl(serverUrl);
  return new Promise((resolve) => {
    chrome.storage.local.set(
      {
        serverUrl: normalized,
        bridgeToken: (token || "").trim(),
      },
      () => resolve(normalized)
    );
  });
}

export async function setLastSelectedJobId(jobId) {
  return new Promise((resolve) => {
    chrome.storage.local.set({ lastSelectedJobId: jobId }, () => resolve());
  });
}

export function classifyYouTubeUrl(rawUrl) {
  if (!rawUrl) {
    return { valid: false, kind: "invalid", label: "No URL", badgeClass: "invalid", reason: "No URL provided." };
  }
  let parsed;
  try {
    parsed = new URL(rawUrl.trim());
  } catch {
    return { valid: false, kind: "invalid", label: "Invalid URL", badgeClass: "invalid", reason: "Not a valid URL." };
  }

  const host = parsed.hostname.toLowerCase();
  if (!ALLOWED_HOSTS.includes(host)) {
    return {
      valid: false,
      kind: "invalid",
      label: "Not YouTube",
      badgeClass: "invalid",
      reason: "Only YouTube and YouTube Music pages are supported.",
    };
  }

  const listParam = parsed.searchParams.get("list");
  const videoParam =
    parsed.searchParams.get("v") ||
    (host === "youtu.be" ? parsed.pathname.slice(1) : null);
  const pathname = parsed.pathname;

  if (videoParam && listParam) {
    return {
      valid: true,
      kind: "choice",
      label: "Song in Playlist",
      badgeClass: "playlist",
      hasChoice: true,
      defaultMode: "song",
    };
  }
  if (listParam) {
    if (listParam.startsWith("OLAK5uy_") || pathname.includes("/browse/MPREb")) {
      return {
        valid: true,
        kind: "album",
        label: "Album",
        badgeClass: "album",
        hasChoice: false,
        defaultMode: "playlist",
      };
    }
    return {
      valid: true,
      kind: "playlist",
      label: "Playlist",
      badgeClass: "playlist",
      hasChoice: false,
      defaultMode: "playlist",
    };
  }
  if (videoParam || pathname.includes("/watch")) {
    return {
      valid: true,
      kind: "song",
      label: "Song / Video",
      badgeClass: "song",
      hasChoice: false,
      defaultMode: "song",
    };
  }

  return {
    valid: false,
    kind: "invalid",
    label: "YouTube Page",
    badgeClass: "invalid",
    reason: "Current page doesn't appear to be a song, album, or playlist.",
  };
}

export function resolveSubmission(rawUrl, classification, chosenMode) {
  const mode = classification.hasChoice
    ? chosenMode || "song"
    : classification.defaultMode || "playlist";
  let effectiveUrl = rawUrl.trim();
  if (classification.hasChoice && mode === "song") {
    try {
      const u = new URL(effectiveUrl);
      u.searchParams.delete("list");
      u.searchParams.delete("index");
      effectiveUrl = u.toString();
    } catch {
      // keep raw
    }
  }
  return { url: effectiveUrl, mode };
}

async function apiRequest(path, options = {}, customSettings = null) {
  const settings = customSettings || (await getSettings());
  const url = `${settings.serverUrl}${path}`;
  const headers = { ...(options.headers || {}) };
  if (settings.token) {
    headers["x-bridge-token"] = settings.token;
  }
  if (options.body && !headers["Content-Type"]) {
    headers["Content-Type"] = "application/json";
  }

  let res;
  try {
    res = await fetch(url, { ...options, headers });
  } catch (err) {
    const error = new Error(
      `Cannot reach server at ${settings.serverUrl}. Check that the server is running and the URL in Settings is correct.`
    );
    error.code = "UNREACHABLE";
    throw error;
  }

  if (res.status === 401) {
    const error = new Error("Token rejected (HTTP 401). Check your Bridge Token in Settings.");
    error.code = "UNAUTHORIZED";
    throw error;
  }

  const data = await res.json().catch(() => ({}));
  if (!res.ok) {
    const error = new Error(data.error || `Server error (HTTP ${res.status})`);
    error.code = "HTTP_ERROR";
    error.status = res.status;
    throw error;
  }
  return data;
}

export async function getHealth(customSettings = null) {
  return apiRequest("/health", { method: "GET" }, customSettings);
}

export async function getJobs(customSettings = null) {
  const data = await apiRequest("/jobs", { method: "GET" }, customSettings);
  return data.jobs || [];
}

export async function getJob(id, customSettings = null) {
  return apiRequest(`/jobs/${id}`, { method: "GET" }, customSettings);
}

export async function getLog(id, since = 0, customSettings = null) {
  return apiRequest(`/jobs/${id}/log?since=${since}`, { method: "GET" }, customSettings);
}

export async function submitJob(url, mode = "playlist", customSettings = null) {
  const data = await apiRequest(
    "/jobs",
    {
      method: "POST",
      body: JSON.stringify({ url, mode }),
    },
    customSettings
  );
  notifyBadgeRefresh();
  return data;
}

export async function cancelJob(id, customSettings = null) {
  const data = await apiRequest(`/jobs/${id}/cancel`, { method: "POST" }, customSettings);
  notifyBadgeRefresh();
  return data;
}

export async function clearJobs(customSettings = null) {
  const data = await apiRequest("/jobs/clear", { method: "POST" }, customSettings);
  notifyBadgeRefresh();
  return data;
}

export async function scanPlex(customSettings = null) {
  return apiRequest("/plex/scan", { method: "POST" }, customSettings);
}

export function notifyBadgeRefresh() {
  try {
    chrome.runtime.sendMessage({ type: "refresh-badge" }).catch(() => {});
  } catch {
    // ignore if background worker isn't listening yet
  }
}

export async function openPopoutWindow(jobId = null) {
  if (jobId) {
    await setLastSelectedJobId(jobId);
  }
  const sessionData = await chrome.storage.session.get(["popoutWindowId"]).catch(() => ({}));
  if (sessionData && sessionData.popoutWindowId) {
    try {
      const win = await chrome.windows.get(sessionData.popoutWindowId);
      if (win && win.id) {
        await chrome.windows.update(win.id, { focused: true });
        return win;
      }
    } catch {
      // Window was closed; create a new one below
    }
  }

  const url = chrome.runtime.getURL("sidepanel.html?popout=1");
  const created = await chrome.windows.create({
    url,
    type: "popup",
    width: 440,
    height: 680,
    focused: true,
  });
  if (created && created.id) {
    await chrome.storage.session.set({ popoutWindowId: created.id }).catch(() => {});
  }
  return created;
}
