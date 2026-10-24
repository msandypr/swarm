// Swarm Extension Popup Controller

const elStatusPill = document.getElementById('status-pill');
const elStatusText = document.getElementById('status-text');
const elToggleIntercept = document.getElementById('toggle-intercept');
const elInputPort = document.getElementById('input-port');
const elInputToken = document.getElementById('input-token');
const elBtnToggleToken = document.getElementById('btn-toggle-token');
const elBtnSave = document.getElementById('btn-save');
const elBtnOpenApp = document.getElementById('btn-open-app');

document.addEventListener('DOMContentLoaded', async () => {
  const settings = await getSettings();

  elToggleIntercept.checked = settings.interceptEnabled !== false;
  elInputPort.value = settings.port || 52417;
  elInputToken.value = settings.token || '';

  await checkDaemonStatus(settings.port);

  // Toggle Intercept on-the-fly
  elToggleIntercept.addEventListener('change', () => {
    chrome.storage.local.set({ interceptEnabled: elToggleIntercept.checked });
  });

  // Toggle Token Visibility
  elBtnToggleToken.addEventListener('click', () => {
    if (elInputToken.type === 'password') {
      elInputToken.type = 'text';
      elBtnToggleToken.textContent = '🔒';
    } else {
      elInputToken.type = 'password';
      elBtnToggleToken.textContent = '👁';
    }
  });

  // Save Settings
  elBtnSave.addEventListener('click', async () => {
    const port = parseInt(elInputPort.value) || 52417;
    const token = elInputToken.value.trim();

    await chrome.storage.local.set({ port, token });
    elBtnSave.textContent = 'Saved!';
    setTimeout(() => (elBtnSave.textContent = 'Save & Connect'), 1500);

    await checkDaemonStatus(port);
  });

  // Open App in Browser
  elBtnOpenApp.addEventListener('click', () => {
    const port = parseInt(elInputPort.value) || 52417;
    const token = elInputToken.value.trim();
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
    const res = await fetch(`http://127.0.0.1:${port}/api/ping`, {
      signal: controller.signal,
    });
    clearTimeout(timeout);

    if (res.ok) {
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
