// Swarm Extension Popup Controller — Zero-Config Mode

const elStatusPill = document.getElementById('status-pill');
const elStatusText = document.getElementById('status-text');
const elToggleIntercept = document.getElementById('toggle-intercept');
const elPortInfo = document.getElementById('port-info');
const elBtnOpenApp = document.getElementById('btn-open-app');

document.addEventListener('DOMContentLoaded', async () => {
  const settings = await getSettings();

  elToggleIntercept.checked = settings.interceptEnabled !== false;
  elPortInfo.textContent = `127.0.0.1:${settings.port || 52417}`;

  await checkDaemonStatus(settings.port || 52417);

  // Toggle Intercept on-the-fly
  elToggleIntercept.addEventListener('change', () => {
    chrome.storage.local.set({ interceptEnabled: elToggleIntercept.checked });
  });

  // Open App in Browser
  elBtnOpenApp.addEventListener('click', async () => {
    const current = await getSettings();
    const port = current.port || 52417;
    const token = current.token || '';
    const url = token ? `http://127.0.0.1:${port}/?t=${encodeURIComponent(token)}` : `http://127.0.0.1:${port}/`;
    chrome.tabs.create({ url });
  });
});

async function checkDaemonStatus(port) {
  elStatusPill.className = 'status-pill offline';
  elStatusText.textContent = 'Checking...';

  try {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 800);
    const res = await fetch(`http://127.0.0.1:${port}/api/ext/handshake`, {
      signal: controller.signal,
    });
    clearTimeout(timeout);

    if (res.ok) {
      const data = await res.json();
      if (data.token) {
        chrome.storage.local.set({ token: data.token, port: data.port });
      }
      elStatusPill.className = 'status-pill online';
      elStatusText.textContent = 'Online';
    } else {
      elStatusPill.className = 'status-pill offline';
      elStatusText.textContent = 'Offline';
    }
  } catch {
    elStatusPill.className = 'status-pill offline';
    elStatusText.textContent = 'Offline';
  }
}

function getSettings() {
  return new Promise((resolve) => {
    chrome.storage.local.get(
      {
        interceptEnabled: true,
        port: 52417,
        token: '',
      },
      (settings) => resolve(settings)
    );
  });
}
