// Swarm Download Manager — Studio Minimalist Core

interface SegmentProgress {
  index: number;
  start: number;
  end: number;
  downloaded: number;
}

interface DownloadItem {
  id: string;
  url: string;
  filename: string;
  output_path: string;
  total_size?: number;
  downloaded_bytes: number;
  status: 'Queued' | 'Downloading' | 'Paused' | 'Completed' | 'Failed';
  speed_bytes_per_sec?: number;
  eta_seconds?: number;
  segments?: SegmentProgress[];
  created_at?: number;
}

type CategoryType =
  | 'all'
  | 'incomplete'
  | 'complete'
  | 'video'
  | 'audio'
  | 'compressed'
  | 'document'
  | 'application'
  | 'misc';

// Global State
let downloads: Map<string, DownloadItem> = new Map();
let selectedId: string | null = null;
let currentCategory: CategoryType = 'all';
let authToken: string = '';
let activePort: number = 52417;

function init() {
  extractToken();
  setupEventListeners();
  fetchInitialData();
  connectEventSource();
}

function extractToken() {
  try {
    const urlParams = new URLSearchParams(window.location.search);
    const tokenParam = urlParams.get('t');

    if (tokenParam) {
      authToken = tokenParam;
      localStorage.setItem('swarm_token', tokenParam);
      window.history.replaceState({}, document.title, window.location.pathname);
    } else {
      authToken = localStorage.getItem('swarm_token') || '';
    }
  } catch (err) {
    console.warn('Error reading token from storage/url:', err);
  }
}

function setupEventListeners() {
  const modalUrl = document.getElementById('modal-url') as HTMLElement | null;
  const formUrl = document.getElementById('form-new-url') as HTMLFormElement | null;
  const inputUrl = document.getElementById('input-url') as HTMLTextAreaElement | null;
  const selectSockets = document.getElementById('select-sockets') as HTMLSelectElement | null;

  // New URL Modal
  document.getElementById('btn-new-url')?.addEventListener('click', () => {
    modalUrl?.classList.add('open');
    inputUrl?.focus();

    if (navigator.clipboard && navigator.clipboard.readText) {
      navigator.clipboard.readText().then((text) => {
        if (text && inputUrl && (text.startsWith('http://') || text.startsWith('https://'))) {
          inputUrl.value = text;
        }
      }).catch(() => {});
    }
  });

  document.getElementById('btn-close-url-dialog')?.addEventListener('click', () => {
    modalUrl?.classList.remove('open');
  });
  document.getElementById('btn-cancel-url')?.addEventListener('click', () => {
    modalUrl?.classList.remove('open');
  });

  formUrl?.addEventListener('submit', async (e) => {
    e.preventDefault();
    if (!inputUrl) return;
    const url = inputUrl.value.trim();
    const connections = selectSockets ? parseInt(selectSockets.value) || 8 : 8;
    if (!url) return;

    await addDownload(url, connections);
    modalUrl?.classList.remove('open');
    inputUrl.value = '';
  });

  // Action Buttons
  document.getElementById('btn-resume')?.addEventListener('click', () => {
    if (selectedId) triggerAction(selectedId, 'resume');
  });

  document.getElementById('btn-stop')?.addEventListener('click', () => {
    if (selectedId) triggerAction(selectedId, 'pause');
  });

  document.getElementById('btn-delete')?.addEventListener('click', () => {
    if (selectedId) {
      if (confirm('Delete this download?')) {
        deleteDownload(selectedId, true);
      }
    }
  });

  // Browsers Dialog
  const modalBrowsers = document.getElementById('modal-browsers');
  document.getElementById('btn-browsers')?.addEventListener('click', () => {
    modalBrowsers?.classList.add('open');
  });
  document.getElementById('btn-close-browsers-dialog')?.addEventListener('click', () => {
    modalBrowsers?.classList.remove('open');
  });
  document.getElementById('btn-close-browsers')?.addEventListener('click', () => {
    modalBrowsers?.classList.remove('open');
  });
  const installStatus = document.getElementById('ext-install-status');

  document.getElementById('btn-install-edge')?.addEventListener('click', async () => {
    const btn = document.getElementById('btn-install-edge') as HTMLButtonElement | null;
    if (btn) btn.textContent = 'Installing...';
    try {
      const headers: Record<string, string> = { 'Content-Type': 'application/json' };
      if (authToken) headers['Authorization'] = `Bearer ${authToken}`;
      const res = await fetch('/api/ext/install', {
        method: 'POST',
        headers,
        body: JSON.stringify({ browser: 'edge' }),
      });
      if (res.ok) {
        if (btn) btn.textContent = '✓ Installed';
        if (installStatus) {
          installStatus.innerHTML = '<span style="color:var(--status-complete)">Edge extension registered! Click "Add extension" on the browser prompt.</span>';
        }
      } else {
        if (btn) btn.textContent = 'Try Again';
      }
    } catch {
      if (btn) btn.textContent = 'Error';
    }
  });

  document.getElementById('btn-install-chrome')?.addEventListener('click', async () => {
    const btn = document.getElementById('btn-install-chrome') as HTMLButtonElement | null;
    if (btn) btn.textContent = 'Installing...';
    try {
      const headers: Record<string, string> = { 'Content-Type': 'application/json' };
      if (authToken) headers['Authorization'] = `Bearer ${authToken}`;
      const res = await fetch('/api/ext/install', {
        method: 'POST',
        headers,
        body: JSON.stringify({ browser: 'chrome' }),
      });
      if (res.ok) {
        if (btn) btn.textContent = '✓ Installed';
        if (installStatus) {
          installStatus.innerHTML = '<span style="color:var(--status-complete)">Chrome extension registered! Click "Add extension" on the browser prompt.</span>';
        }
      } else {
        if (btn) btn.textContent = 'Try Again';
      }
    } catch {
      if (btn) btn.textContent = 'Error';
    }
  });

  // Settings Dialog
  const modalSettings = document.getElementById('modal-settings');
  document.getElementById('btn-settings')?.addEventListener('click', () => {
    modalSettings?.classList.add('open');
  });
  document.getElementById('btn-close-settings-dialog')?.addEventListener('click', () => {
    modalSettings?.classList.remove('open');
  });
  document.getElementById('btn-close-settings')?.addEventListener('click', () => {
    modalSettings?.classList.remove('open');
  });

  // About Dialog
  const modalAbout = document.getElementById('modal-about');
  document.getElementById('btn-about')?.addEventListener('click', () => {
    modalAbout?.classList.add('open');
  });
  document.getElementById('btn-close-about-dialog')?.addEventListener('click', () => {
    modalAbout?.classList.remove('open');
  });
  document.getElementById('btn-close-about')?.addEventListener('click', () => {
    modalAbout?.classList.remove('open');
  });

  // Sidebar navigation
  document.querySelectorAll('[data-category]').forEach((el) => {
    el.addEventListener('click', (e) => {
      e.stopPropagation();
      document.querySelectorAll('[data-category]').forEach((c) => c.classList.remove('active'));
      el.classList.add('active');

      const cat = el.getAttribute('data-category');
      if (
        cat === 'all' ||
        cat === 'incomplete' ||
        cat === 'complete' ||
        cat === 'video' ||
        cat === 'audio' ||
        cat === 'compressed' ||
        cat === 'document' ||
        cat === 'application' ||
        cat === 'misc'
      ) {
        currentCategory = cat;
      }
      renderTable();
    });
  });
}

async function fetchInitialData() {
  try {
    const resPing = await fetch('/api/ping');
    if (resPing.ok) {
      const pingData = await resPing.json();
      activePort = pingData.port;
      const activePortEl = document.getElementById('active-port');
      if (activePortEl) activePortEl.textContent = activePort.toString();
      const settingsPortEl = document.getElementById('settings-port') as HTMLInputElement | null;
      if (settingsPortEl) settingsPortEl.value = activePort.toString();
    }

    const headers: Record<string, string> = {};
    if (authToken) headers['Authorization'] = `Bearer ${authToken}`;

    const res = await fetch('/api/downloads', { credentials: 'include', headers });
    if (res.ok) {
      const list: DownloadItem[] = await res.json();
      downloads.clear();
      list.forEach((item) => downloads.set(item.id, item));
      renderTable();
    }
  } catch (err) {
    console.error('Fetch error:', err);
  }
}

async function addDownload(url: string, connections: number) {
  try {
    const headers: Record<string, string> = { 'Content-Type': 'application/json' };
    if (authToken) headers['Authorization'] = `Bearer ${authToken}`;

    const res = await fetch('/api/downloads', {
      method: 'POST',
      headers,
      body: JSON.stringify({ url, connections }),
    });

    if (res.ok) {
      await fetchInitialData();
    } else {
      alert('Failed: ' + (await res.text()));
    }
  } catch {
    alert('Network error adding download');
  }
}

async function triggerAction(id: string, action: 'pause' | 'resume' | 'cancel') {
  try {
    const headers: Record<string, string> = { 'Content-Type': 'application/json' };
    if (authToken) headers['Authorization'] = `Bearer ${authToken}`;

    await fetch(`/api/downloads/${id}`, {
      method: 'PATCH',
      headers,
      body: JSON.stringify({ action }),
    });

    const item = downloads.get(id);
    if (item && action === 'pause') {
      item.status = 'Paused';
      item.speed_bytes_per_sec = 0;
      updateRowLive(item);
      updateSelectionButtons();
    }
  } catch (err) {
    console.error('Action error:', err);
  }
}

async function deleteDownload(id: string, deleteFile: boolean = false) {
  try {
    const headers: Record<string, string> = {};
    if (authToken) headers['Authorization'] = `Bearer ${authToken}`;

    await fetch(`/api/downloads/${id}?delete_file=${deleteFile}`, {
      method: 'DELETE',
      headers,
    });

    downloads.delete(id);
    if (selectedId === id) selectedId = null;
    renderTable();
  } catch (err) {
    console.error('Delete error:', err);
  }
}

function connectEventSource() {
  const url = authToken ? `/api/events?t=${encodeURIComponent(authToken)}` : '/api/events';
  const es = new EventSource(url);

  es.addEventListener('progress', (e) => {
    try {
      const data = JSON.parse(e.data);
      const id = data.id;

      let item = downloads.get(id);
      if (!item) {
        item = {
          id: data.id,
          url: '',
          filename: data.filename,
          output_path: '',
          total_size: data.total_size,
          downloaded_bytes: data.downloaded_bytes,
          status: data.status,
          speed_bytes_per_sec: data.speed_bytes_per_sec,
          eta_seconds: data.eta_seconds,
          segments: data.segments,
        };
        downloads.set(id, item);
        renderTable();
      } else {
        item.filename = data.filename || item.filename;
        item.total_size = data.total_size || item.total_size;
        item.downloaded_bytes = data.downloaded_bytes;
        item.status = data.status;
        item.speed_bytes_per_sec = data.speed_bytes_per_sec;
        item.eta_seconds = data.eta_seconds;
        item.segments = data.segments;
        updateRowLive(item);
      }

      updateGlobalMetrics();
    } catch (err) {
      console.error('SSE error:', err);
    }
  });
}

function renderTable() {
  const elTableBody = document.getElementById('table-body') as HTMLElement | null;
  const elEmptyState = document.getElementById('empty-state') as HTMLElement | null;
  const elCountAll = document.getElementById('count-all') as HTMLElement | null;
  const elCountActive = document.getElementById('count-active') as HTMLElement | null;
  const elCountComplete = document.getElementById('count-complete') as HTMLElement | null;
  const elFooterCount = document.getElementById('footer-count') as HTMLElement | null;

  if (!elTableBody) return;

  const items = Array.from(downloads.values());

  const activeCount = items.filter((i) => i.status === 'Downloading').length;
  const completeCount = items.filter((i) => i.status === 'Completed').length;

  if (elCountAll) elCountAll.textContent = items.length.toString();
  if (elCountActive) elCountActive.textContent = activeCount.toString();
  if (elCountComplete) elCountComplete.textContent = completeCount.toString();
  if (elFooterCount) elFooterCount.textContent = `${items.length} items`;

  const filtered = items.filter((item) => {
    if (currentCategory === 'complete') return item.status === 'Completed';
    if (currentCategory === 'incomplete') return item.status !== 'Completed';

    if (currentCategory === 'all') return true;

    const ext = (item.filename.split('.').pop() || '').toLowerCase();
    if (currentCategory === 'video') return ['mp4', 'mkv', 'avi', 'mov', 'webm'].includes(ext);
    if (currentCategory === 'audio') return ['mp3', 'wav', 'flac', 'aac'].includes(ext);
    if (currentCategory === 'compressed') return ['zip', 'rar', '7z', 'tar', 'gz', 'iso'].includes(ext);
    if (currentCategory === 'document') return ['pdf', 'doc', 'docx', 'txt', 'epub'].includes(ext);
    if (currentCategory === 'application') return ['exe', 'msi', 'bin'].includes(ext);
    if (currentCategory === 'misc') {
      return !['mp4', 'mkv', 'avi', 'mov', 'webm', 'mp3', 'wav', 'flac', 'aac', 'zip', 'rar', '7z', 'tar', 'gz', 'iso', 'pdf', 'doc', 'docx', 'txt', 'epub', 'exe', 'msi', 'bin'].includes(ext);
    }
    return true;
  });

  if (filtered.length === 0) {
    elTableBody.innerHTML = '';
    if (elEmptyState) elEmptyState.style.display = 'flex';
  } else {
    if (elEmptyState) elEmptyState.style.display = 'none';
    elTableBody.innerHTML = '';
    filtered.forEach((item) => {
      const tr = document.createElement('tr');
      tr.className = `data-row ${selectedId === item.id ? 'selected' : ''}`;
      tr.id = `row-${item.id}`;

      tr.addEventListener('click', () => {
        document.querySelectorAll('.data-row').forEach((r) => r.classList.remove('selected'));
        tr.classList.add('selected');
        selectedId = item.id;
        updateSelectionButtons();
      });

      tr.innerHTML = getRowHtml(item);
      elTableBody.appendChild(tr);
    });
  }

  updateSelectionButtons();
  updateGlobalMetrics();
}

function updateRowLive(item: DownloadItem) {
  const tr = document.getElementById(`row-${item.id}`);
  if (!tr) return;

  tr.innerHTML = getRowHtml(item);
}

function getRowHtml(item: DownloadItem): string {
  const iconSvg = getFileIconSvg(item.filename);
  const sizeStr = item.total_size ? formatBytes(item.total_size) : formatBytes(item.downloaded_bytes);

  let statusText = 'Complete';
  let statusClass = 'complete';

  if (item.status === 'Downloading') {
    const pct = item.total_size ? Math.min(100, Math.round((item.downloaded_bytes / item.total_size) * 100)) : 0;
    statusText = `${pct}%`;
    statusClass = 'downloading';
  } else if (item.status === 'Paused') {
    const pct = item.total_size ? Math.min(100, Math.round((item.downloaded_bytes / item.total_size) * 100)) : 0;
    statusText = `Paused (${pct}%)`;
    statusClass = 'paused';
  } else if (item.status === 'Failed') {
    statusText = 'Error';
    statusClass = 'failed';
  }

  const speedStr = item.status === 'Downloading' && item.speed_bytes_per_sec ? formatBytes(item.speed_bytes_per_sec) + '/s' : '—';
  const etaStr = item.status === 'Downloading' && item.eta_seconds !== undefined ? formatEta(item.eta_seconds) : '—';
  const dateStr = formatDate(item.created_at);

  const progressBar = item.status === 'Downloading' || item.status === 'Paused'
    ? getInlineProgressHtml(item)
    : '';

  return `
    <td class="col-name">
      <div class="cell-file">
        ${iconSvg}
        <span class="file-text" title="${escapeHtml(item.filename)}">${escapeHtml(item.filename || 'Starting download...')}</span>
      </div>
    </td>
    <td class="col-size mono">${sizeStr}</td>
    <td class="col-status">
      <div class="status-cell">
        <span class="status-label ${statusClass}">${statusText}</span>
        ${progressBar}
      </div>
    </td>
    <td class="col-speed mono">${speedStr}</td>
    <td class="col-eta mono">${etaStr}</td>
    <td class="col-date mono">${dateStr}</td>
  `;
}

function getInlineProgressHtml(item: DownloadItem): string {
  const p = item.total_size ? Math.min(100, (item.downloaded_bytes / item.total_size) * 100) : 0;
  const pausedClass = item.status === 'Paused' ? 'paused' : '';
  return `
    <div class="inline-progress">
      <div class="progress-track ${pausedClass}" style="width:${p}%"></div>
    </div>
  `;
}

function updateSelectionButtons() {
  const btnResume = document.getElementById('btn-resume');
  const btnStop = document.getElementById('btn-stop');
  const btnDelete = document.getElementById('btn-delete');

  if (!btnResume || !btnStop || !btnDelete) return;

  if (!selectedId) {
    btnResume.classList.add('disabled');
    btnStop.classList.add('disabled');
    btnDelete.classList.add('disabled');
    return;
  }

  const item = downloads.get(selectedId);
  if (!item) {
    btnResume.classList.add('disabled');
    btnStop.classList.add('disabled');
    btnDelete.classList.add('disabled');
    return;
  }

  btnDelete.classList.remove('disabled');

  if (item.status === 'Downloading') {
    btnResume.classList.add('disabled');
    btnStop.classList.remove('disabled');
  } else if (item.status === 'Paused') {
    btnResume.classList.remove('disabled');
    btnStop.classList.add('disabled');
  } else {
    btnResume.classList.add('disabled');
    btnStop.classList.add('disabled');
  }
}

function updateGlobalMetrics() {
  const elGlobalSpeed = document.getElementById('global-speed');
  if (!elGlobalSpeed) return;

  let totalSpeed = 0;
  for (const item of downloads.values()) {
    if (item.status === 'Downloading' && item.speed_bytes_per_sec) {
      totalSpeed += item.speed_bytes_per_sec;
    }
  }

  elGlobalSpeed.textContent = totalSpeed > 0 ? `${formatBytes(totalSpeed)}/s` : '0.0 KB/s';
}

function getFileIconSvg(filename?: string): string {
  const ext = (filename?.split('.').pop() || '').toLowerCase();
  if (['mp4', 'mkv', 'avi', 'mov', 'webm'].includes(ext)) {
    return `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><rect width="18" height="18" x="3" y="3" rx="2"/><path d="M7 3v18"/><path d="M3 7.5h4"/><path d="M3 12h18"/><path d="M3 16.5h4"/><path d="M17 3v18"/><path d="M17 7.5h4"/><path d="M17 16.5h4"/></svg>`;
  }
  if (['mp3', 'wav', 'flac', 'aac'].includes(ext)) {
    return `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>`;
  }
  if (['zip', 'rar', '7z', 'tar', 'gz', 'iso'].includes(ext)) {
    return `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><rect width="20" height="5" x="2" y="3" rx="1"/><path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8"/><path d="M10 12h4"/></svg>`;
  }
  if (['pdf', 'doc', 'docx', 'txt', 'epub'].includes(ext)) {
    return `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="M10 9H8"/><path d="M16 13H8"/><path d="M16 17H8"/></svg>`;
  }
  if (['exe', 'msi', 'bin'].includes(ext)) {
    return `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><rect x="14" y="14" width="4" height="6" rx="2"/><rect x="6" y="4" width="4" height="6" rx="2"/><path d="M6 20h4"/><path d="M14 10h4"/><path d="M6 14h2v6"/><path d="M14 4h2v6"/></svg>`;
  }
  return `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/></svg>`;
}

function formatBytes(bytes: number): string {
  if (bytes <= 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}

function formatEta(seconds: number): string {
  if (seconds <= 0) return '0s';
  if (seconds < 60) return `${seconds}s`;
  const mins = Math.floor(seconds / 60);
  const secs = seconds % 60;
  if (mins < 60) return `${mins}m ${secs}s`;
  const hours = Math.floor(mins / 60);
  return `${hours}h ${mins % 60}m`;
}

function formatDate(timestamp?: number): string {
  const d = timestamp ? new Date(timestamp * 1000) : new Date();
  const months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
  const m = months[d.getMonth()];
  const day = d.getDate();
  const h = String(d.getHours()).padStart(2, '0');
  const min = String(d.getMinutes()).padStart(2, '0');
  return `${m} ${day} ${h}:${min}`;
}

function escapeHtml(text?: string): string {
  if (!text) return '';
  return text.replace(/[&<>"']/g, (m) => ({
    '&': '&amp;',
    '<': '&lt;',
    '>': '&gt;',
    '"': '&quot;',
    "'": '&#039;',
  }[m] || m));
}

window.addEventListener('DOMContentLoaded', init);
if (document.readyState === 'complete' || document.readyState === 'interactive') {
  init();
}
