// Swarm Download Manager — Background Service Worker (Manifest V3)

const DEFAULT_SETTINGS = {
  interceptEnabled: true,
  port: 52417,
  token: '',
  minFileSizeKB: 0,
};

// Setup context menus on install
chrome.runtime.onInstalled.addListener(() => {
  chrome.storage.local.get(DEFAULT_SETTINGS, (settings) => {
    chrome.storage.local.set(settings);
  });

  chrome.contextMenus.create({
    id: 'swarm-download-link',
    title: 'Download with Swarm',
    contexts: ['link'],
  });

  chrome.contextMenus.create({
    id: 'swarm-download-media',
    title: 'Download Media with Swarm',
    contexts: ['image', 'video', 'audio'],
  });
});

// Handle Context Menu clicks
chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  const targetUrl = info.linkUrl || info.srcUrl;
  if (!targetUrl) return;

  const settings = await getSettings();
  const cookies = await getCookiesForUrl(targetUrl);

  await sendToDaemon(targetUrl, tab?.url || '', settings, cookies, '');
});

// Intercept browser downloads
chrome.downloads.onCreated.addListener(async (downloadItem) => {
  try {
    const settings = await getSettings();
    if (!settings.interceptEnabled) return;

    const url = downloadItem.finalUrl || downloadItem.url;
    if (!url) return;

    // Skip special and unsupported protocols
    if (url.startsWith('blob:') || url.startsWith('data:') || url.startsWith('chrome:') || url.startsWith('edge:') || url.startsWith('about:')) {
      return;
    }

    // Skip small files if minimum threshold configured
    if (settings.minFileSizeKB > 0 && downloadItem.fileSize > 0 && downloadItem.fileSize < settings.minFileSizeKB * 1024) {
      return;
    }

    const cookies = await getCookiesForUrl(url);
    const referrer = downloadItem.referrer || '';
    const suggestedFilename = downloadItem.filename ? downloadItem.filename.split(/[\\/]/).pop() : '';

    const accepted = await sendToDaemon(url, referrer, settings, cookies, suggestedFilename);
    if (accepted) {
      // Daemon accepted; cancel browser download and erase from browser history
      chrome.downloads.cancel(downloadItem.id, () => {
        chrome.downloads.erase({ id: downloadItem.id });
      });
    }
  } catch (err) {
    // Fail-safe: if any error occurs, let the browser proceed normally
    console.warn('Swarm intercept failed, falling back to browser download:', err);
  }
});

async function sendToDaemon(url, referrer, settings, cookieString, filename) {
  const endpoint = `http://127.0.0.1:${settings.port}/api/ext/add`;
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), 1200);

  const headers = {
    'Content-Type': 'application/json',
  };
  if (settings.token) {
    headers['Authorization'] = `Bearer ${settings.token}`;
  }

  try {
    const res = await fetch(endpoint, {
      method: 'POST',
      headers,
      signal: controller.signal,
      body: JSON.stringify({
        url,
        referrer,
        user_agent: navigator.userAgent,
        cookie: cookieString || undefined,
        suggested_filename: filename || undefined,
      }),
    });

    clearTimeout(timeoutId);
    return res.ok;
  } catch (err) {
    clearTimeout(timeoutId);
    return false; // Daemon is offline or rejected
  }
}

async function getCookiesForUrl(url) {
  try {
    const cookies = await chrome.cookies.getAll({ url });
    return cookies.map((c) => `${c.name}=${c.value}`).join('; ');
  } catch {
    return '';
  }
}

function getSettings() {
  return new Promise((resolve) => {
    chrome.storage.local.get(DEFAULT_SETTINGS, (settings) => {
      resolve(settings);
    });
  });
}
