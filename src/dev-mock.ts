// Mock IPC untuk mengembangkan UI di browser biasa (`npm run dev`, tanpa Tauri).
// Hanya dimuat saat import.meta.env.DEV dan tidak berjalan di dalam Tauri — tidak ikut build produksi.

import { mockIPC } from '@tauri-apps/api/mocks';
import type { AppSettings, RepoInfo, ScanReport } from './lib/types';

const base = { pid: null, cpuPercent: 0, memoryMb: 0, notice: null, phase: null, url: null, scanSummary: null, createdAt: 0 };

const repos: RepoInfo[] = [
  { ...base, id: 'a', name: 'landing-page', sourceKind: 'git', source: 'https://github.com/acme/landing', stack: 'Vite', status: 'running', port: 5173, url: 'http://localhost:5173/', pid: 4120, cpuPercent: 12, memoryMb: 310, scanVerdict: 'clean', scanSummary: 'Tidak ditemukan indikasi berbahaya' },
  { ...base, id: 'b', name: 'dashboard-next', sourceKind: 'git', source: 'https://github.com/acme/dashboard', stack: 'Next.js', status: 'running', port: 3001, url: 'http://localhost:3001', pid: 5530, cpuPercent: 86, memoryMb: 1320, notice: "Port 3000 sedang dipakai repo 'api-server'. Dialihkan otomatis ke port 3001.", scanVerdict: 'clean', scanSummary: 'Tidak ditemukan indikasi berbahaya' },
  { ...base, id: 'c', name: 'free-template', sourceKind: 'archive', source: 'free-template.zip', stack: 'HTML statis', status: 'stopped', port: null, scanVerdict: 'suspicious', scanSummary: '2 temuan berisiko tinggi, 1 sedang' },
  { ...base, id: 'd', name: 'blog-laravel', sourceKind: 'git', source: 'https://github.com/acme/blog', stack: 'Laravel', status: 'installing', port: 8000, phase: 'Menginstal dependensi Composer', scanVerdict: 'clean', scanSummary: 'Tidak ditemukan indikasi berbahaya' },
];

const suspiciousReport: ScanReport = {
  verdict: 'suspicious',
  summary: '2 temuan berisiko tinggi, 1 sedang',
  note: 'Repo ini terdeteksi tidak normal',
  scannedFiles: 214,
  antivirus: 'Windows Defender: tidak ada ancaman',
  scannedAt: 0,
  findings: [
    { severity: 'high', rule: 'disguised_pe', title: 'Executable Windows disamarkan dengan ekstensi lain', file: 'assets/img/logo.png', line: null, detail: "'logo.png' berisi program .exe" },
    { severity: 'high', rule: 'ps_encoded', title: 'PowerShell dengan perintah ter-encode (base64)', file: 'js/setup.bat', line: 3, detail: 'powershell -NoP -W Hidden -enc SQBFAFgAIAAoAE4AZQB3AC0ATwBi…' },
    { severity: 'medium', rule: 'exfil_webhook', title: 'Webhook Discord/Telegram (sering dipakai untuk eksfiltrasi)', file: 'js/main.js', line: 88, detail: "fetch('https://discord.com/api/webhooks/1234/…')" },
  ],
};

const settings: AppSettings = {
  cpuThresholdPercent: 80, memoryThresholdMb: 1024, sustainSecs: 30, autoKillAfterSecs: 0, snoozeSecs: 600,
  idleTimeoutMins: 30, idleCpuPercent: 1, cpuCoresLimit: 0, cpuRateLimitPercent: 0, memoryHardLimitMb: 0,
  lowPriority: true, cleanCacheOnStop: false, allowInstallScripts: false, antivirusScan: true,
  minimizeToTray: false, closeToTray: true,
};

export function installMock() {
  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case 'list_repos': return repos;
        case 'get_settings': return settings;
        case 'update_settings': return a.settings;
        case 'cpu_count': return 8;
        case 'validate_git_url': return String(a.url).startsWith('https://') ? null : 'Hanya URL https:// yang diizinkan';
        case 'get_scan_report': return a.id === 'c' ? suspiciousReport : { ...suspiciousReport, verdict: 'clean', note: null, summary: 'Tidak ditemukan indikasi berbahaya', findings: [] };
        case 'get_logs': return ['[repolaunch] Stack: Vite · port 5173', '$ npm run dev -- --port 5173 --strictPort --host 127.0.0.1', '', '  VITE v6.0.0  ready in 412 ms', '', '  ➜  Local:   http://localhost:5173/'];
        case 'clean_cache': return 42.5;
        default: return null;
      }
    },
    { shouldMockEvents: true },
  );
}
