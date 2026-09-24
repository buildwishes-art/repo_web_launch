// Dialog berbasis Promise: komponen memanggil `await showScanReport(...)` / `await confirm(...)`,
// dan <Dialogs /> di App.svelte merender dialog yang aktif.

import type { RepoInfo, ScanReport } from './types';

export type ScanDialogResult = 'close' | 'run' | 'delete';

interface ScanDialogState {
  repoId: string;
  repoName: string;
  report: ScanReport | null;
  loading: boolean;
  offerRun: boolean;
  offerDelete: boolean;
  resolve: (r: ScanDialogResult) => void;
}

interface ConfirmState {
  title: string;
  message: string;
  confirmLabel: string;
  danger: boolean;
  resolve: (ok: boolean) => void;
}

export const dialogs = $state({
  scan: null as ScanDialogState | null,
  confirm: null as ConfirmState | null,
  logs: null as RepoInfo | null,
  addRepo: false,
});

export function showScanReport(opts: {
  repoId: string;
  repoName: string;
  report?: ScanReport | null;
  offerRun?: boolean;
  offerDelete?: boolean;
}): Promise<ScanDialogResult> {
  return new Promise((resolve) => {
    dialogs.scan = {
      repoId: opts.repoId,
      repoName: opts.repoName,
      report: opts.report ?? null,
      loading: opts.report === undefined,
      offerRun: opts.offerRun ?? false,
      offerDelete: opts.offerDelete ?? false,
      resolve,
    };
  });
}

export function closeScanReport(result: ScanDialogResult) {
  const d = dialogs.scan;
  dialogs.scan = null;
  d?.resolve(result);
}

export function confirm(opts: { title: string; message: string; confirmLabel?: string; danger?: boolean }): Promise<boolean> {
  return new Promise((resolve) => {
    dialogs.confirm = {
      title: opts.title,
      message: opts.message,
      confirmLabel: opts.confirmLabel ?? 'OK',
      danger: opts.danger ?? false,
      resolve,
    };
  });
}

export function closeConfirm(ok: boolean) {
  const d = dialogs.confirm;
  dialogs.confirm = null;
  d?.resolve(ok);
}
