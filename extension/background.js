// Swarm Download Manager — Background Service Worker (Manifest V3)
// Zero-Configuration Auto-Pairing Mode

const DEFAULT_SETTINGS = {
  interceptEnabled: true,
  port: 52417,
  token: '',
};

// Setup context menus and perform initial handshake on install or browser startup
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

  autoHandshake();
});

chrome.runtime.onStartup.addListener(() => {
  autoHandshake();
});

// Auto-Handshake with daemon to synchronize token and port without user action
async function autoHandshake() {
  const settings = await getSettings();
  const port = settings.port || 52417;

  try {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 1000);
    const res = await fetch(`http://127.0.0.1:${port}/api/ext/handshake`, {
      signal: controller.signal,
    });
    clearTimeout(timeout);

    if (res.ok) {
      const data = await res.json();
      if (data.token) {
        chrome.storage.local.set({ token: data.token, port: data.port });
      }
    }
  } catch {
    // Daemon offline, will retry on next intercept attempt
  }
}

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

    // Skip internal browser protocols
    if (url.startsWith('blob:') || url.startsWith('data:') || url.startsWith('chrome:') || url.startsWith('edge:') || url.startsWith('about:')) {
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
  let port = settings.port || 52417;
  let token = settings.token;

  // If token is missing, attempt quick auto-handshake first
  if (!token) {
    try {
      const hRes = await fetch(`http://127.0.0.1:${port}/api/ext/handshake`);
      if (hRes.ok) {
        const hData = await hRes.json();
        token = hData.token;
        chrome.storage.local.set({ token: hData.token, port: hData.port });
      }
    } catch {
      // continue without token; Origin header will authenticate extension
    }
  }

  const endpoint = `http://127.0.0.1:${port}/api/ext/add`;
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), 1200);

  const headers = {
    'Content-Type': 'application/json',
  };
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
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
  } catch {
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
