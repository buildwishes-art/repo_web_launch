// Cermin dari src-tauri/src/engine/types.rs (serde camelCase). Ubah keduanya bersamaan.

export type SourceKind = 'git' | 'archive';
export type RepoStatus = 'stopped' | 'installing' | 'starting' | 'running' | 'crashed';
export type Severity = 'info' | 'medium' | 'high';
export type ScanVerdict = 'notScanned' | 'clean' | 'suspicious';
export type EventKind =
  | 'statusChanged'
  | 'metrics'
  | 'resourceAlert'
  | 'alertCleared'
  | 'autoKilled'
  | 'idleStopped'
  | 'urlDetected';

export interface RepoInfo {
  id: string;
  name: string;
  sourceKind: SourceKind;
  source: string;
  stack: string;
  createdAt: number;
  status: RepoStatus;
  port: number | null;
  url: string | null;
  pid: number | null;
  cpuPercent: number;
  memoryMb: number;
  notice: string | null;
  phase: string | null;
  scanVerdict: ScanVerdict;
  scanSummary: string | null;
}

export interface ScanFinding {
  severity: Severity;
  rule: string;
  title: string;
  file: string | null;
  line: number | null;
  detail: string;
}

export interface ScanReport {
  verdict: ScanVerdict;
  summary: string;
  note: string | null;
  findings: ScanFinding[];
  scannedFiles: number;
  antivirus: string;
  scannedAt: number;
}

export interface CoreEvent {
  kind: EventKind;
  repoId: string;
  repoName: string;
  status: RepoStatus | null;
  port: number | null;
  url: string | null;
  cpuPercent: number | null;
  memoryMb: number | null;
  message: string | null;
}

export interface AppSettings {
  cpuThresholdPercent: number;
  memoryThresholdMb: number;
  sustainSecs: number;
  autoKillAfterSecs: number;
  snoozeSecs: number;
  idleTimeoutMins: number;
  idleCpuPercent: number;
  cpuCoresLimit: number;
  cpuRateLimitPercent: number;
  memoryHardLimitMb: number;
  lowPriority: boolean;
  cleanCacheOnStop: boolean;
  allowInstallScripts: boolean;
  antivirusScan: boolean;
  minimizeToTray: boolean;
  closeToTray: boolean;
}

export const SUSPICIOUS_NOTE = 'Repo ini terdeteksi tidak normal';
