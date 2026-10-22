import { getHealth, getSettings } from "./api.js";

const ALARM_NAME = "poll-active-jobs";

async function updateBadge() {
  try {
    const settings = await getSettings();
    if (!settings.token) {
      await chrome.action.setBadgeText({ text: "" });
      return;
    }
    const health = await getHealth(settings);
    const active = (health.running || 0) + (health.queued || 0);
    if (active > 0) {
      await chrome.action.setBadgeBackgroundColor({ color: "#2b6cb0" });
      await chrome.action.setBadgeText({ text: String(active) });
    } else {
      await chrome.action.setBadgeText({ text: "" });
    }
  } catch {
    await chrome.action.setBadgeText({ text: "" });
  }
}

chrome.runtime.onInstalled.addListener(() => {
  chrome.alarms.create(ALARM_NAME, { periodInMinutes: 0.5 });
  updateBadge();
});

chrome.runtime.onStartup.addListener(() => {
  chrome.alarms.create(ALARM_NAME, { periodInMinutes: 0.5 });
  updateBadge();
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === ALARM_NAME) {
    updateBadge();
  }
});

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg && msg.type === "refresh-badge") {
    updateBadge().then(() => sendResponse({ ok: true }));
    return true;
  }
  return false;
});
