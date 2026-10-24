// Swarm Download Manager — Web UI App Core

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

// Global State
let downloads: Map<String, DownloadItem> = new Map();
let currentFilter: 'all' | 'downloading' | 'completed' | 'paused' = 'all';
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
    // Clean URL query without refreshing page
    window.history.replaceState({}, document.title, window.location.pathname);
  } else {
    authToken = localStorage.getItem('swarm_token') || '';
  }

  tokenInput.value = authToken || 'No token active';
}

function setupEventListeners() {
  // Modal Add
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

  // Drawer Settings
  document.getElementById('btn-settings')?.addEventListener('click', () => {
    drawerSettings.classList.add('open');
  });
  document.getElementById('btn-close-drawer')?.addEventListener('click', () => {
    drawerSettings.classList.remove('open');
  });

  // Copy Token
  btnCopyToken?.addEventListener('click', () => {
    if (authToken) {
      navigator.clipboard.writeText(authToken);
      btnCopyToken.textContent = 'Copied!';
      setTimeout(() => (btnCopyToken.textContent = 'Copy'), 2000);
    }
  });

  // Filter Tabs
  document.querySelectorAll('.tab-btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      document.querySelectorAll('.tab-btn').forEach((b) => b.classList.remove('active'));
      btn.classList.add('active');
      const filterVal = btn.getAttribute('data-filter');
      if (filterVal === 'all' || filterVal === 'downloading' || filterVal === 'completed' || filterVal === 'paused') {
        currentFilter = filterVal;
      }
      renderList();
    });
  });

  // Pause All
  document.getElementById('btn-pause-all')?.addEventListener('click', async () => {
    for (const [id, item] of downloads) {
      if (item.status === 'Downloading') {
        await triggerAction(id.toString(), 'pause');
      }
    }
  });

  // Clear Completed
  document.getElementById('btn-clear-completed')?.addEventListener('click', async () => {
    for (const [id, item] of downloads) {
      if (item.status === 'Completed') {
        await deleteDownload(id.toString(), false);
      }
    }
  });
}

function openAddModal() {
  modalAdd.classList.add('open');
  inputUrl.focus();

  // Try reading clipboard automatically for convenience
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
  } catch (err) {
    alert('Network error when adding download');
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
      renderCard(item);
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
    elConnStatus.innerHTML = '<span class="status-dot"></span><span class="status-text">Connected</span>';
    elConnStatus.style.borderColor = 'rgba(16, 185, 129, 0.3)';
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
    elConnStatus.innerHTML = '<span class="status-dot" style="background:#f59e0b;box-shadow:none"></span><span class="status-text" style="color:#f59e0b">Reconnecting</span>';
  };
}

// Render Functions
function renderList() {
  const items = Array.from(downloads.values());

  // Update Badges
  const activeCount = items.filter((i) => i.status === 'Downloading').length;
  const completedCount = items.filter((i) => i.status === 'Completed').length;
  const pausedCount = items.filter((i) => i.status === 'Paused').length;

  elBadgeAll.textContent = items.length.toString();
  elBadgeActive.textContent = activeCount.toString();
  elBadgeCompleted.textContent = completedCount.toString();
  elBadgePaused.textContent = pausedCount.toString();

  // Filter
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

  const icon = getFileIcon(item.filename);
  const totalStr = item.total_size ? formatBytes(item.total_size) : 'Unknown size';
  const downloadedStr = formatBytes(item.downloaded_bytes);
  const percent = item.total_size ? Math.min(100, Math.round((item.downloaded_bytes / item.total_size) * 100)) : 0;
  const speedStr = item.status === 'Downloading' && item.speed_bytes_per_sec ? formatBytes(item.speed_bytes_per_sec) + '/s' : '--';
  const etaStr = item.status === 'Downloading' && item.eta_seconds !== undefined ? formatEta(item.eta_seconds) : '--';

  card.innerHTML = `
    <div class="card-top">
      <div class="card-file-info">
        <div class="file-icon">${icon}</div>
        <div class="file-names">
          <span class="file-name" title="${escapeHtml(item.filename)}">${escapeHtml(item.filename || 'Initializing download...')}</span>
          <span class="file-url" title="${escapeHtml(item.url)}">${escapeHtml(item.url)}</span>
        </div>
      </div>
      <div class="card-actions">
        ${
          item.status === 'Downloading'
            ? `<button class="btn-icon" data-action="pause" data-id="${item.id}" title="Pause">⏸</button>`
            : item.status === 'Paused'
            ? `<button class="btn-icon" data-action="resume" data-id="${item.id}" title="Resume">▶</button>`
            : ''
        }
        <button class="btn-icon danger" data-action="delete" data-id="${item.id}" title="Delete">🗑</button>
      </div>
    </div>

    <div class="card-metrics">
      <div class="metric-left">
        <span class="metric-size">${downloadedStr} of ${totalStr} (${percent}%)</span>
        <span class="metric-status" style="color:${getStatusColor(item.status)}">${item.status}</span>
      </div>
      <div class="metric-right">
        <span class="metric-speed">${speedStr}</span>
        <span class="metric-eta">ETA: ${etaStr}</span>
      </div>
    </div>

    <div class="progress-bar-container" id="bar-${item.id}">
      ${renderSegmentsHtml(item)}
    </div>
  `;

  // Attach button events
  card.querySelectorAll('[data-action]').forEach((btn) => {
    btn.addEventListener('click', (e) => {
      const target = e.currentTarget as HTMLElement;
      const action = target.getAttribute('data-action');
      const id = target.getAttribute('data-id');
      if (!id) return;
      if (action === 'delete') {
        if (confirm('Delete this download?')) deleteDownload(id, true);
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

  const totalStr = item.total_size ? formatBytes(item.total_size) : 'Unknown size';
  const downloadedStr = formatBytes(item.downloaded_bytes);
  const percent = item.total_size ? Math.min(100, Math.round((item.downloaded_bytes / item.total_size) * 100)) : 0;
  const speedStr = item.status === 'Downloading' && item.speed_bytes_per_sec ? formatBytes(item.speed_bytes_per_sec) + '/s' : '--';
  const etaStr = item.status === 'Downloading' && item.eta_seconds !== undefined ? formatEta(item.eta_seconds) : '--';

  const elFileName = card.querySelector('.file-name');
  if (elFileName && item.filename) elFileName.textContent = item.filename;

  const elSize = card.querySelector('.metric-size');
  if (elSize) elSize.textContent = `${downloadedStr} of ${totalStr} (${percent}%)`;

  const elSpeed = card.querySelector('.metric-speed');
  if (elSpeed) elSpeed.textContent = speedStr;

  const elEta = card.querySelector('.metric-eta');
  if (elEta) elEta.textContent = `ETA: ${etaStr}`;

  const elStatus = card.querySelector('.metric-status') as HTMLElement;
  if (elStatus) {
    elStatus.textContent = item.status;
    elStatus.style.color = getStatusColor(item.status);
  }

  const elBar = document.getElementById(`bar-${item.id}`);
  if (elBar) {
    elBar.innerHTML = renderSegmentsHtml(item);
  }
}

function renderSegmentsHtml(item: DownloadItem): string {
  if (item.status === 'Completed') {
    return `<div class="segment-chunk" style="flex:1"><div class="segment-fill progress-completed" style="width:100%"></div></div>`;
  }

  if (!item.segments || item.segments.length === 0) {
    const p = item.total_size ? Math.min(100, (item.downloaded_bytes / item.total_size) * 100) : 0;
    return `<div class="segment-chunk" style="flex:1"><div class="segment-fill" style="width:${p}%"></div></div>`;
  }

  return item.segments
    .map((seg) => {
      const segSize = seg.end - seg.start + 1;
      const segPercent = segSize > 0 ? Math.min(100, (seg.downloaded / segSize) * 100) : 0;
      return `
        <div class="segment-chunk" style="flex:${segSize}">
          <div class="segment-fill" style="width:${segPercent}%"></div>
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
    case 'Downloading': return 'var(--status-active)';
    case 'Completed': return 'var(--status-completed)';
    case 'Paused': return 'var(--status-paused)';
    case 'Failed': return 'var(--status-failed)';
    default: return 'var(--text-muted)';
  }
}

function getFileIcon(filename?: string): string {
  if (!filename) return '📄';
  const ext = filename.split('.').pop()?.toLowerCase();
  switch (ext) {
    case 'zip': case 'rar': case '7z': case 'tar': case 'gz': case 'iso': return '📦';
    case 'mp4': case 'mkv': case 'avi': case 'mov': case 'webm': return '🎬';
    case 'mp3': case 'flac': case 'wav': case 'aac': return '🎵';
    case 'pdf': case 'doc': case 'docx': case 'epub': return '📑';
    case 'exe': case 'msi': return '⚙️';
    case 'jpg': case 'jpeg': case 'png': case 'gif': case 'webp': case 'svg': return '🖼️';
    default: return '📄';
  }
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

// Kickoff
window.addEventListener('DOMContentLoaded', init);
