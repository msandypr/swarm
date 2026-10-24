// Swarm Download Manager — Studio Dark UI Controller

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

type FilterType = 'all' | 'downloading' | 'completed' | 'paused';
type ActionType = 'pause' | 'resume' | 'cancel' | 'delete';

// Global State
let downloads: Map<string, DownloadItem> = new Map();
let currentFilter: FilterType = 'all';
let authToken: string = '';
let activePort: number = 52417;

// DOM Elements
const elDownloadList = document.getElementById('download-list') as HTMLDivElement;
const elEmptyState = document.getElementById('empty-state') as HTMLDivElement;
const elGlobalSpeed = document.getElementById('global-speed') as HTMLSpanElement;
const elConnStatus = document.getElementById('connection-status') as HTMLDivElement;

const elBadgeAll = document.getElementById('badge-all') as HTMLSpanElement;
const elBadgeActive = document.getElementById('badge-active') as HTMLSpanElement;
const elBadgeCompleted = document.getElementById('badge-completed') as HTMLSpanElement;
const elBadgePaused = document.getElementById('badge-paused') as HTMLSpanElement;

// Modal & Drawer
const modalAdd = document.getElementById('modal-add') as HTMLDivElement;
const formAdd = document.getElementById('form-add-download') as HTMLFormElement;
const inputUrl = document.getElementById('input-url') as HTMLTextAreaElement;
const selectConnections = document.getElementById('select-connections') as HTMLSelectElement;

const drawerSettings = document.getElementById('drawer-settings') as HTMLDivElement;
const tokenInput = document.getElementById('token-input') as HTMLInputElement;
const btnCopyToken = document.getElementById('btn-copy-token') as HTMLButtonElement;
const activePortEl = document.getElementById('active-port') as HTMLElement;

// Initialize App
function init() {
  extractToken();
  setupEventListeners();
  fetchInitialData();
  connectEventSource();
}

function extractToken() {
  const urlParams = new URLSearchParams(window.location.search);
  const tokenParam = urlParams.get('t');

  if (tokenParam) {
    authToken = tokenParam;
    localStorage.setItem('swarm_token', tokenParam);
    window.history.replaceState({}, document.title, window.location.pathname);
  } else {
    authToken = localStorage.getItem('swarm_token') || '';
  }

  tokenInput.value = authToken || 'No token active';
}

function setupEventListeners() {
  document.getElementById('btn-add-modal')?.addEventListener('click', openAddModal);
  document.getElementById('btn-empty-add')?.addEventListener('click', openAddModal);
  document.getElementById('btn-close-modal')?.addEventListener('click', closeAddModal);
  document.getElementById('btn-cancel-modal')?.addEventListener('click', closeAddModal);

  formAdd?.addEventListener('submit', async (e) => {
    e.preventDefault();
    const url = inputUrl.value.trim();
    const connections = parseInt(selectConnections.value) || 8;
    if (!url) return;

    await addDownload(url, connections);
    closeAddModal();
    inputUrl.value = '';
  });

  document.getElementById('btn-settings')?.addEventListener('click', () => {
    drawerSettings.classList.add('open');
  });
  document.getElementById('btn-close-drawer')?.addEventListener('click', () => {
    drawerSettings.classList.remove('open');
  });

  btnCopyToken?.addEventListener('click', () => {
    if (authToken) {
      navigator.clipboard.writeText(authToken);
      btnCopyToken.textContent = 'Copied';
      setTimeout(() => (btnCopyToken.textContent = 'Copy'), 2000);
    }
  });

  document.querySelectorAll('.filter-tab').forEach((btn) => {
    btn.addEventListener('click', () => {
      document.querySelectorAll('.filter-tab').forEach((b) => b.classList.remove('active'));
      btn.classList.add('active');
      const filterVal = btn.getAttribute('data-filter');
      if (filterVal === 'all' || filterVal === 'downloading' || filterVal === 'completed' || filterVal === 'paused') {
        currentFilter = filterVal;
      }
      renderList();
    });
  });

  document.getElementById('btn-pause-all')?.addEventListener('click', async () => {
    for (const [id, item] of downloads) {
      if (item.status === 'Downloading') {
        await triggerAction(id, 'pause');
      }
    }
  });

  document.getElementById('btn-clear-completed')?.addEventListener('click', async () => {
    for (const [id, item] of downloads) {
      if (item.status === 'Completed') {
        await deleteDownload(id, false);
      }
    }
  });
}

function openAddModal() {
  modalAdd.classList.add('open');
  inputUrl.focus();

  if (navigator.clipboard && navigator.clipboard.readText) {
    navigator.clipboard.readText().then((text) => {
      if (text && (text.startsWith('http://') || text.startsWith('https://'))) {
        inputUrl.value = text;
      }
    }).catch(() => {});
  }
}

function closeAddModal() {
  modalAdd.classList.remove('open');
}

// API Calls
async function fetchInitialData() {
  try {
    const resPing = await fetch('/api/ping');
    if (resPing.ok) {
      const pingData = await resPing.json();
      activePort = pingData.port;
      activePortEl.textContent = activePort.toString();
    }

    const headers: Record<string, string> = {};
    if (authToken) headers['Authorization'] = `Bearer ${authToken}`;

    const res = await fetch('/api/downloads', { credentials: 'include', headers });
    if (res.ok) {
      const list: DownloadItem[] = await res.json();
      downloads.clear();
      list.forEach((item) => downloads.set(item.id, item));
      renderList();
    }
  } catch (err) {
    console.error('Failed to fetch initial downloads:', err);
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
      alert('Failed to start download: ' + (await res.text()));
    }
  } catch {
    alert('Network error when adding download');
  }
}

async function triggerAction(id: string, action: ActionType) {
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
      updateCardLive(item);
    }
  } catch (err) {
    console.error('Failed to trigger action:', err);
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
    renderList();
  } catch (err) {
    console.error('Failed to delete download:', err);
  }
}

// SSE Progress Event Stream
function connectEventSource() {
  const url = authToken ? `/api/events?t=${encodeURIComponent(authToken)}` : '/api/events';
  const es = new EventSource(url);

  es.onopen = () => {
    elConnStatus.innerHTML = '<span class="status-indicator"></span><span class="status-label">Online</span>';
  };

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
        renderList();
      } else {
        item.filename = data.filename || item.filename;
        item.total_size = data.total_size || item.total_size;
        item.downloaded_bytes = data.downloaded_bytes;
        item.status = data.status;
        item.speed_bytes_per_sec = data.speed_bytes_per_sec;
        item.eta_seconds = data.eta_seconds;
        item.segments = data.segments;
        updateCardLive(item);
      }

      updateGlobalMetrics();
    } catch (err) {
      console.error('SSE parse error:', err);
    }
  });

  es.onerror = () => {
    elConnStatus.innerHTML = '<span class="status-indicator" style="background:#fbbf24"></span><span class="status-label" style="color:#fbbf24">Reconnecting</span>';
  };
}

// Render Functions
function renderList() {
  const items = Array.from(downloads.values());

  const activeCount = items.filter((i) => i.status === 'Downloading').length;
  const completedCount = items.filter((i) => i.status === 'Completed').length;
  const pausedCount = items.filter((i) => i.status === 'Paused').length;

  elBadgeAll.textContent = items.length.toString();
  elBadgeActive.textContent = activeCount.toString();
  elBadgeCompleted.textContent = completedCount.toString();
  elBadgePaused.textContent = pausedCount.toString();

  const filtered = items.filter((item) => {
    if (currentFilter === 'downloading') return item.status === 'Downloading';
    if (currentFilter === 'completed') return item.status === 'Completed';
    if (currentFilter === 'paused') return item.status === 'Paused';
    return true;
  });

  if (filtered.length === 0) {
    elDownloadList.innerHTML = '';
    elEmptyState.style.display = 'flex';
    return;
  }

  elEmptyState.style.display = 'none';
  elDownloadList.innerHTML = '';
  filtered.forEach((item) => {
    elDownloadList.appendChild(createCardElement(item));
  });

  updateGlobalMetrics();
}

function createCardElement(item: DownloadItem): HTMLElement {
  const card = document.createElement('div');
  card.className = 'download-card';
  card.id = `card-${item.id}`;

  const iconSvg = getFileIconSvg(item.filename);
  const totalStr = item.total_size ? formatBytes(item.total_size) : 'Unknown';
  const downloadedStr = formatBytes(item.downloaded_bytes);
  const percent = item.total_size ? Math.min(100, Math.round((item.downloaded_bytes / item.total_size) * 100)) : 0;
  const speedStr = item.status === 'Downloading' && item.speed_bytes_per_sec ? formatBytes(item.speed_bytes_per_sec) + '/s' : '--';
  const etaStr = item.status === 'Downloading' && item.eta_seconds !== undefined ? formatEta(item.eta_seconds) : '--';

  card.innerHTML = `
    <div class="card-top">
      <div class="file-meta-col">
        <div class="file-type-icon">${iconSvg}</div>
        <div class="file-title-wrap">
          <span class="file-title" title="${escapeHtml(item.filename)}">${escapeHtml(item.filename || 'Starting download...')}</span>
          <span class="file-source" title="${escapeHtml(item.url)}">${escapeHtml(item.url)}</span>
        </div>
      </div>
      <div class="card-action-bar">
        ${
          item.status === 'Downloading'
            ? `<button class="btn-tool" data-action="pause" data-id="${item.id}" title="Pause">
                <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4" width="4" height="16" rx="1"/><rect x="14" y="4" width="4" height="16" rx="1"/></svg>
               </button>`
            : item.status === 'Paused'
            ? `<button class="btn-tool" data-action="resume" data-id="${item.id}" title="Resume">
                <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><polygon points="6 4 20 12 6 20 6 4"/></svg>
               </button>`
            : ''
        }
        <button class="btn-tool remove" data-action="delete" data-id="${item.id}" title="Delete">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
        </button>
      </div>
    </div>

    <div class="metrics-row">
      <div class="metrics-left">
        <span>${downloadedStr} of ${totalStr} (${percent}%)</span>
        <span style="color:${getStatusColor(item.status)}">${item.status}</span>
      </div>
      <div class="metrics-right">
        <span class="metric-rate">${speedStr}</span>
        <span>ETA: ${etaStr}</span>
      </div>
    </div>

    <div class="segment-bar" id="bar-${item.id}">
      ${renderSegmentsHtml(item)}
    </div>
  `;

  card.querySelectorAll('[data-action]').forEach((btn) => {
    btn.addEventListener('click', (e) => {
      const target = e.currentTarget as HTMLElement;
      const action = target.getAttribute('data-action');
      const id = target.getAttribute('data-id');
      if (!id) return;

      if (action === 'delete') {
        if (confirm('Delete this download record?')) deleteDownload(id, true);
      } else if (action === 'pause' || action === 'resume' || action === 'cancel') {
        triggerAction(id, action);
      }
    });
  });

  return card;
}

function updateCardLive(item: DownloadItem) {
  const card = document.getElementById(`card-${item.id}`);
  if (!card) return;

  const totalStr = item.total_size ? formatBytes(item.total_size) : 'Unknown';
  const downloadedStr = formatBytes(item.downloaded_bytes);
  const percent = item.total_size ? Math.min(100, Math.round((item.downloaded_bytes / item.total_size) * 100)) : 0;
  const speedStr = item.status === 'Downloading' && item.speed_bytes_per_sec ? formatBytes(item.speed_bytes_per_sec) + '/s' : '--';
  const etaStr = item.status === 'Downloading' && item.eta_seconds !== undefined ? formatEta(item.eta_seconds) : '--';

  const elFileName = card.querySelector('.file-title');
  if (elFileName && item.filename) elFileName.textContent = item.filename;

  const elMetricsLeft = card.querySelector('.metrics-left');
  if (elMetricsLeft) {
    elMetricsLeft.innerHTML = `
      <span>${downloadedStr} of ${totalStr} (${percent}%)</span>
      <span style="color:${getStatusColor(item.status)}">${item.status}</span>
    `;
  }

  const elRate = card.querySelector('.metric-rate');
  if (elRate) elRate.textContent = speedStr;

  const elBar = document.getElementById(`bar-${item.id}`);
  if (elBar) {
    elBar.innerHTML = renderSegmentsHtml(item);
  }
}

function renderSegmentsHtml(item: DownloadItem): string {
  if (item.status === 'Completed') {
    return `<div class="segment-track" style="flex:1"><div class="segment-solid-fill done" style="width:100%"></div></div>`;
  }

  if (!item.segments || item.segments.length === 0) {
    const p = item.total_size ? Math.min(100, (item.downloaded_bytes / item.total_size) * 100) : 0;
    const fillClass = item.status === 'Paused' ? 'paused' : '';
    return `<div class="segment-track" style="flex:1"><div class="segment-solid-fill ${fillClass}" style="width:${p}%"></div></div>`;
  }

  const fillClass = item.status === 'Paused' ? 'paused' : '';
  return item.segments
    .map((seg) => {
      const segSize = seg.end - seg.start + 1;
      const segPercent = segSize > 0 ? Math.min(100, (seg.downloaded / segSize) * 100) : 0;
      return `
        <div class="segment-track" style="flex:${segSize}">
          <div class="segment-solid-fill ${fillClass}" style="width:${segPercent}%"></div>
        </div>
      `;
    })
    .join('');
}

function updateGlobalMetrics() {
  let totalSpeed = 0;
  for (const item of downloads.values()) {
    if (item.status === 'Downloading' && item.speed_bytes_per_sec) {
      totalSpeed += item.speed_bytes_per_sec;
    }
  }
  elGlobalSpeed.textContent = totalSpeed > 0 ? `${formatBytes(totalSpeed)}/s` : '0.0 KB/s';
}

// Helpers
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

function getStatusColor(status: string): string {
  switch (status) {
    case 'Downloading': return 'var(--fill-active)';
    case 'Completed': return 'var(--fill-completed)';
    case 'Paused': return 'var(--fill-paused)';
    case 'Failed': return 'var(--fill-failed)';
    default: return 'var(--text-muted)';
  }
}

function getFileIconSvg(filename?: string): string {
  const ext = filename?.split('.').pop()?.toLowerCase() || '';

  if (['zip', 'rar', '7z', 'tar', 'gz', 'iso'].includes(ext)) {
    // Archive
    return `<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"></path><polyline points="3.27 6.96 12 12.01 20.73 6.96"></polyline><line x1="12" y1="22.08" x2="12" y2="12"></line></svg>`;
  }

  if (['mp4', 'mkv', 'avi', 'mov', 'webm', 'mp3', 'wav', 'flac'].includes(ext)) {
    // Media
    return `<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="2" y="2" width="20" height="20" rx="2.18" ry="2.18"></rect><line x1="7" y1="2" x2="7" y2="22"></line><line x1="17" y1="2" x2="17" y2="22"></line><line x1="2" y1="12" x2="22" y2="12"></line><line x1="2" y1="7" x2="7" y2="7"></line><line x1="2" y1="17" x2="7" y2="17"></line><line x1="17" y1="17" x2="22" y2="17"></line><line x1="17" y1="7" x2="22" y2="7"></line></svg>`;
  }

  if (['exe', 'msi', 'bin'].includes(ext)) {
    // Binary
    return `<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="4" y="4" width="16" height="16" rx="2"></rect><rect x="9" y="9" width="6" height="6"></rect><line x1="9" y1="1" x2="9" y2="4"></line><line x1="15" y1="1" x2="15" y2="4"></line><line x1="9" y1="20" x2="9" y2="23"></line><line x1="15" y1="20" x2="15" y2="23"></line><line x1="20" y1="9" x2="23" y2="9"></line><line x1="20" y1="14" x2="23" y2="14"></line><line x1="1" y1="9" x2="4" y2="9"></line><line x1="1" y1="14" x2="4" y2="14"></line></svg>`;
  }

  // Document default
  return `<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><polyline points="14 2 14 8 20 8"></polyline><line x1="16" y1="13" x2="8" y2="13"></line><line x1="16" y1="17" x2="8" y2="17"></line></svg>`;
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
