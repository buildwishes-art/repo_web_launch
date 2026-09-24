// Pembungkus bertipe untuk command Tauri (src-tauri/src/commands.rs).
// Argumen camelCase otomatis dipetakan Tauri ke parameter snake_case di Rust.

import { invoke } from '@tauri-apps/api/core';
import type { AppSettings, RepoInfo, ScanReport } from './types';

export const api = {
  listRepos: () => invoke<RepoInfo[]>('list_repos'),
  addRepoFromGit: (url: string, name: string | null) => invoke<RepoInfo>('add_repo_from_git', { url, name }),
  addRepoFromArchive: (path: string, name: string | null) => invoke<RepoInfo>('add_repo_from_archive', { path, name }),
  deleteRepo: (id: string) => invoke<void>('delete_repo', { id }),
  validateGitUrl: (url: string) => invoke<string | null>('validate_git_url', { url }),

  startRepo: (id: string, allowSuspicious = false) => invoke<RepoInfo>('start_repo', { id, allowSuspicious }),
  stopRepo: (id: string) => invoke<void>('stop_repo', { id }),
  getLogs: (id: string) => invoke<string[]>('get_logs', { id }),
  openRepoUrl: (id: string) => invoke<void>('open_repo_url', { id }),
  respondToAlert: (id: string, kill: boolean) => invoke<void>('respond_to_alert', { id, kill }),

  getSettings: () => invoke<AppSettings>('get_settings'),
  updateSettings: (settings: AppSettings) => invoke<AppSettings>('update_settings', { settings }),
  cleanCache: (repoId: string | null) => invoke<number>('clean_cache', { repoId }),
  cpuCount: () => invoke<number>('cpu_count'),

  getScanReport: (id: string) => invoke<ScanReport | null>('get_scan_report', { id }),
  rescanRepo: (id: string) => invoke<ScanReport>('rescan_repo', { id }),
};

/** Error dari command Rust tiba sebagai string. */
export function errorText(e: unknown): string {
  return typeof e === 'string' ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
