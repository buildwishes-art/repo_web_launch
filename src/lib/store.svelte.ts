// State global reaktif (Svelte 5 runes). Sumber kebenaran tetap di Rust; store ini hanya
// cache tampilan yang diperbarui lewat event "core-event" dan refresh daftar repo.

import { listen } from '@tauri-apps/api/event';
import { api, errorText } from './api';
import type { AppSettings, CoreEvent, RepoInfo, RepoStatus } from './types';

export interface Metric {
  cpu: number;
  memMb: number;
}

export interface Toast {
  id: number;
  text: string;
  kind: 'info' | 'success' | 'error';
}

export const app = $state({
  repos: [] as RepoInfo[],
  metrics: {} as Record<string, Metric>,
  busy: {} as Record<string, boolean>,
  settings: null as AppSettings | null,
  cpuCount: 1,
  loaded: false,
  loadError: null as string | null,
  view: 'repos' as 'repos' | 'settings',
});

export const toasts = $state({ items: [] as Toast[] });

let toastSeq = 0;
export function toast(text: string, kind: Toast['kind'] = 'info', ms = 5000) {
  const id = ++toastSeq;
  toasts.items.push({ id, text, kind });
  setTimeout(() => dismissToast(id), ms);
}
export function dismissToast(id: number) {
  toasts.items = toasts.items.filter((t) => t.id !== id);
}

export function isActive(s: RepoStatus) {
  return s === 'installing' || s === 'starting' || s === 'running';
}

export async function refresh() {
  try {
    app.repos = await api.listRepos();
    app.loadError = null;
  } catch (e) {
    app.loadError = errorText(e);
  } finally {
    app.loaded = true;
  }
}

// Satu start memicu beberapa event berurutan → gabungkan jadi satu refresh.
let refreshTimer: ReturnType<typeof setTimeout> | undefined;
function scheduleRefresh() {
  clearTimeout(refreshTimer);
  refreshTimer = setTimeout(refresh, 150);
}

/** Menandai repo "sibuk" selama aksi berjalan (menonaktifkan tombol). */
export async function withBusy<T>(id: string, task: () => Promise<T>): Promise<T> {
  app.busy[id] = true;
  try {
    return await task();
  } finally {
    delete app.busy[id];
  }
}

function handle(ev: CoreEvent) {
  switch (ev.kind) {
    case 'metrics':
      app.metrics[ev.repoId] = { cpu: ev.cpuPercent ?? 0, memMb: ev.memoryMb ?? 0 };
      break;
    case 'statusChanged':
    case 'urlDetected':
      if (ev.status === 'crashed' && ev.message) toast(`${ev.repoName}: ${ev.message}`, 'error', 8000);
      else if (ev.kind === 'urlDetected' && ev.message) toast(`${ev.repoName}: ${ev.message}`);
      if (ev.status === 'stopped' || ev.status === 'crashed') delete app.metrics[ev.repoId];
      scheduleRefresh();
      break;
    case 'autoKilled':
    case 'idleStopped':
      toast(ev.message ?? `${ev.repoName} dihentikan`, 'info', 8000);
      delete app.metrics[ev.repoId];
      scheduleRefresh();
      break;
    // resourceAlert / alertCleared ditangani di Rust (toast Windows ber-aksi).
    default:
      break;
  }
}

export async function initStore() {
  await listen<CoreEvent>('core-event', (e) => handle(e.payload));
  const [, settings, cpus] = await Promise.all([refresh(), api.getSettings(), api.cpuCount()]);
  app.settings = settings;
  app.cpuCount = cpus;
}

export function formatMem(mb: number) {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb.toFixed(0)} MB`;
}

export const statusLabel: Record<RepoStatus, string> = {
  stopped: 'Berhenti',
  installing: 'Menginstal',
  starting: 'Memulai',
  running: 'Berjalan',
  crashed: 'Crash',
};
