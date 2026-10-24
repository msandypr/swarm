import {
  createIcons,
  Plus,
  Play,
  Square,
  X,
  Settings,
  Globe,
  Info,
  Power,
  Folder,
  FolderCheck,
  FolderDown,
  Film,
  Music,
  Archive,
  FileText,
  Binary,
  File as FileIcon,
} from 'lucide';

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
  | 'video'
  | 'audio'
  | 'compressed'
  | 'document'
  | 'application'
  | 'misc'
  | 'complete'
  | 'incomplete';

// State
let downloads: Map<string, DownloadItem> = new Map();
let selectedId: string | null = null;
let currentCategory: CategoryType = 'all';
let authToken: string = '';
let activePort: number = 52417;

// DOM Elements
const elTableBody = document.getElementById('table-body') as HTMLTableSectionElement;
const elStatusSummary = document.getElementById('status-summary') as HTMLSpanElement;
const elStatusSpeed = document.getElementById('status-speed') as HTMLSpanElement;

// Top Buttons
const btnResume = document.getElementById('btn-resume') as HTMLButtonElement;
const btnStop = document.getElementById('btn-stop') as HTMLButtonElement;
const btnDelete = document.getElementById('btn-delete') as HTMLButtonElement;

// Dialogs
const modalUrl = document.getElementById('modal-url') as HTMLDivElement;
const formUrl = document.getElementById('form-new-url') as HTMLFormElement;
const inputUrl = document.getElementById('input-url') as HTMLTextAreaElement;
const selectSockets = document.getElementById('select-sockets') as HTMLSelectElement;

const modalBrowsers = document.getElementById('modal-browsers') as HTMLDivElement;
const tokenInput = document.getElementById('token-input') as HTMLInputElement;
const btnCopyToken = document.getElementById('btn-copy-token') as HTMLButtonElement;
const activePortEl = document.getElementById('active-port') as HTMLElement;

const modalAbout = document.getElementById('modal-about') as HTMLDivElement;

function init() {
  extractToken();
  initLucide();
  setupEventListeners();
  fetchInitialData();
  connectEventSource();
}

function initLucide() {
  createIcons({
    icons: {
      Plus,
      Play,
      Square,
      X,
      Settings,
      Globe,
      Info,
      Power,
      Folder,
      FolderCheck,
      FolderDown,
      Film,
      Music,
      Archive,
      FileText,
      Binary,
      File: FileIcon,
    },
  });
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

  tokenInput.value = authToken;
}

function setupEventListeners() {
  // New URL
  document.getElementById('btn-new-url')?.addEventListener('click', () => {
    modalUrl.classList.add('open');
    inputUrl.focus();

    if (navigator.clipboard && navigator.clipboard.readText) {
      navigator.clipboard.readText().then((text) => {
        if (text && (text.startsWith('http://') || text.startsWith('https://'))) {
          inputUrl.value = text;
        }
      }).catch(() => {});
    }
  });

  document.getElementById('btn-close-url-dialog')?.addEventListener('click', () => {
    modalUrl.classList.remove('open');
  });
  document.getElementById('btn-cancel-url')?.addEventListener('click', () => {
    modalUrl.classList.remove('open');
  });

  formUrl.addEventListener('submit', async (e) => {
    e.preventDefault();
    const url = inputUrl.value.trim();
    const connections = parseInt(selectSockets.value) || 8;
    if (!url) return;

    await addDownload(url, connections);
    modalUrl.classList.remove('open');
    inputUrl.value = '';
  });

  // Top action bar actions
  btnResume.addEventListener('click', () => {
    if (selectedId) triggerAction(selectedId, 'resume');
  });

  btnStop.addEventListener('click', () => {
    if (selectedId) triggerAction(selectedId, 'pause');
  });

  btnDelete.addEventListener('click', () => {
    if (selectedId) {
      if (confirm('Delete this download?')) {
        deleteDownload(selectedId, true);
      }
    }
  });

  // Browsers Dialog
  document.getElementById('btn-browsers')?.addEventListener('click', () => {
    modalBrowsers.classList.add('open');
  });
  document.getElementById('btn-close-browsers-dialog')?.addEventListener('click', () => {
    modalBrowsers.classList.remove('open');
  });
  document.getElementById('btn-close-browsers')?.addEventListener('click', () => {
    modalBrowsers.classList.remove('open');
  });

  btnCopyToken.addEventListener('click', () => {
    if (authToken) {
      navigator.clipboard.writeText(authToken);
      btnCopyToken.textContent = 'Copied';
      setTimeout(() => (btnCopyToken.textContent = 'Copy'), 1500);
    }
  });

  // About Dialog
  document.getElementById('btn-about')?.addEventListener('click', () => {
    modalAbout.classList.add('open');
  });
  document.getElementById('btn-close-about-dialog')?.addEventListener('click', () => {
    modalAbout.classList.remove('open');
  });
  document.getElementById('btn-close-about')?.addEventListener('click', () => {
    modalAbout.classList.remove('open');
  });

  // Quit / Close
  document.getElementById('btn-quit')?.addEventListener('click', () => {
    window.close();
  });

  // Tree category selection
  document.querySelectorAll('[data-category]').forEach((el) => {
    el.addEventListener('click', (e) => {
      e.stopPropagation();
      document.querySelectorAll('[data-category]').forEach((c) => c.classList.remove('active'));
      el.classList.add('active');

      const cat = el.getAttribute('data-category');
      if (
        cat === 'all' ||
        cat === 'video' ||
        cat === 'audio' ||
        cat === 'compressed' ||
        cat === 'document' ||
        cat === 'application' ||
        cat === 'misc' ||
        cat === 'complete' ||
        cat === 'incomplete'
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
      activePortEl.textContent = activePort.toString();
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
  const items = Array.from(downloads.values());

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

  elTableBody.innerHTML = '';
  filtered.forEach((item) => {
    const tr = document.createElement('tr');
    tr.className = `download-row ${selectedId === item.id ? 'selected' : ''}`;
    tr.id = `row-${item.id}`;

    tr.addEventListener('click', () => {
      document.querySelectorAll('.download-row').forEach((r) => r.classList.remove('selected'));
      tr.classList.add('selected');
      selectedId = item.id;
      updateSelectionButtons();
    });

    tr.innerHTML = getRowHtml(item);
    elTableBody.appendChild(tr);
  });

  initLucide();
  updateSelectionButtons();
  updateGlobalMetrics();
}

function updateRowLive(item: DownloadItem) {
  const tr = document.getElementById(`row-${item.id}`);
  if (!tr) return;

  tr.innerHTML = getRowHtml(item);
  initLucide();
}

function getRowHtml(item: DownloadItem): string {
  const iconName = getFileLucideIcon(item.filename);
  const sizeStr = item.total_size ? formatBytes(item.total_size) : formatBytes(item.downloaded_bytes);

  let statusText = 'Complete';
  let statusClass = 'status-done';

  if (item.status === 'Downloading') {
    const pct = item.total_size ? Math.min(100, Math.round((item.downloaded_bytes / item.total_size) * 100)) : 0;
    statusText = `Downloading ( ${pct}% )`;
    statusClass = 'status-active';
  } else if (item.status === 'Paused') {
    const pct = item.total_size ? Math.min(100, Math.round((item.downloaded_bytes / item.total_size) * 100)) : 0;
    statusText = `Paused ( ${pct}% )`;
    statusClass = 'status-paused';
  } else if (item.status === 'Failed') {
    statusText = 'Error ( 0% )';
    statusClass = 'status-error';
  }

  const speedStr = item.status === 'Downloading' && item.speed_bytes_per_sec ? formatBytes(item.speed_bytes_per_sec) + '/s' : '';
  const etaStr = item.status === 'Downloading' && item.eta_seconds !== undefined ? formatEta(item.eta_seconds) : '';
  const dateStr = formatDate(item.created_at);

  return `
    <td class="col-filename">
      <div class="file-cell">
        <i data-lucide="${iconName}"></i>
        <span class="file-text" title="${escapeHtml(item.filename)}">${escapeHtml(item.filename || 'Starting download...')}</span>
      </div>
    </td>
    <td class="col-size mono-num">${sizeStr}</td>
    <td class="col-status ${statusClass}">${statusText}</td>
    <td class="col-speed mono-num">${speedStr}</td>
    <td class="col-eta mono-num">${etaStr}</td>
    <td class="col-date mono-num">${dateStr}</td>
  `;
}

function updateSelectionButtons() {
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
  let totalSpeed = 0;
  for (const item of downloads.values()) {
    if (item.status === 'Downloading' && item.speed_bytes_per_sec) {
      totalSpeed += item.speed_bytes_per_sec;
    }
  }

  elStatusSpeed.textContent = totalSpeed > 0 ? `${formatBytes(totalSpeed)}/s` : '0.0 KB/s';
  elStatusSummary.textContent = `${downloads.size} downloads`;
}

function getFileLucideIcon(filename?: string): string {
  const ext = (filename?.split('.').pop() || '').toLowerCase();
  if (['mp4', 'mkv', 'avi', 'mov', 'webm'].includes(ext)) return 'film';
  if (['mp3', 'wav', 'flac', 'aac'].includes(ext)) return 'music';
  if (['zip', 'rar', '7z', 'tar', 'gz', 'iso'].includes(ext)) return 'archive';
  if (['pdf', 'doc', 'docx', 'txt', 'epub'].includes(ext)) return 'file-text';
  if (['exe', 'msi', 'bin'].includes(ext)) return 'binary';
  return 'file';
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
  const sec = String(d.getSeconds()).padStart(2, '0');
  return `${m} ${day} ${h}:${min}:${sec}`;
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
