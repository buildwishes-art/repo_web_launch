<script lang="ts">
  import { api, errorText } from '../lib/api';
  import { closeScanReport, dialogs } from '../lib/dialogs.svelte';
  import { SUSPICIOUS_NOTE, type ScanFinding, type ScanReport } from '../lib/types';
  import Icon from './Icon.svelte';
  import Modal from './Modal.svelte';

  const current = $derived(dialogs.scan);
  let report = $state<ScanReport | null>(null);
  let loadError = $state<string | null>(null);
  let acknowledged = $state(false);

  // Muat laporan dari Rust jika pemanggil tidak menyertakannya.
  $effect(() => {
    const s = dialogs.scan;
    acknowledged = false;
    loadError = null;
    report = s?.report ?? null;
    if (s?.loading) {
      api
        .getScanReport(s.repoId)
        .then((r) => (report = r))
        .catch((e) => (loadError = errorText(e)));
    }
  });

  const suspicious = $derived(report?.verdict === 'suspicious');

  const sevLabel: Record<ScanFinding['severity'], string> = { high: 'TINGGI', medium: 'SEDANG', info: 'INFO' };
</script>

<Modal
  open={!!current}
  wide
  tone={suspicious ? 'danger' : 'default'}
  title={suspicious ? SUSPICIOUS_NOTE : `Hasil scan keamanan · ${current?.repoName ?? ''}`}
  onclose={() => closeScanReport('close')}
>
  {#if loadError}
    <p class="error">{loadError}</p>
  {:else if !report}
    <p class="muted">Repo ini belum pernah dipindai. Scan akan dijalankan otomatis saat Start.</p>
  {:else}
    <div class="verdict" class:bad={suspicious}>
      <Icon name={suspicious ? 'shieldAlert' : 'shieldCheck'} size={28} />
      <div>
        {#if suspicious}
          <strong>Note: {report.note ?? SUSPICIOUS_NOTE}.</strong>
          <p>
            Repo diblokir agar tidak dijalankan. Tinjau temuan di bawah; hapus repo jika Anda tidak mengenal
            sumbernya.
          </p>
        {:else}
          <strong>Tidak ditemukan indikasi berbahaya.</strong>
        {/if}
        <p class="meta">{report.summary} · {report.scannedFiles} file dipindai · {report.antivirus}</p>
      </div>
    </div>

    {#if report.findings.length}
      <ul class="findings">
        {#each report.findings as f, i (i)}
          <li class={f.severity}>
            <span class="sev">{sevLabel[f.severity]}</span>
            <div>
              <strong>{f.title}</strong>
              {#if f.file}
                <span class="loc">{f.file}{f.line ? ` · baris ${f.line}` : ''}</span>
              {/if}
              {#if f.detail}<code>{f.detail}</code>{/if}
            </div>
          </li>
        {/each}
      </ul>
    {/if}

    {#if current?.offerRun && suspicious}
      <label class="ack">
        <input type="checkbox" bind:checked={acknowledged} />
        Saya memahami risikonya dan mempercayai sumber repo ini
      </label>
    {/if}
  {/if}

  {#snippet actions()}
    {#if current?.offerDelete}
      <button class="btn ghost danger" onclick={() => closeScanReport('delete')}><Icon name="trash" size={16} />Hapus repo</button>
    {/if}
    <button class="btn ghost" onclick={() => closeScanReport('close')}>Tutup</button>
    {#if current?.offerRun && suspicious}
      <button class="btn danger-solid" disabled={!acknowledged} onclick={() => closeScanReport('run')}>
        Tetap jalankan
      </button>
    {/if}
  {/snippet}
</Modal>

<style>
  .verdict {
    display: flex;
    gap: 12px;
    padding: 14px;
    border-radius: 12px;
    background: var(--ok-bg);
    color: var(--ok-fg);
  }
  .verdict.bad {
    background: var(--danger-bg);
    color: var(--danger-fg);
  }
  .verdict p {
    margin: 4px 0 0;
  }
  .meta {
    font-size: 0.82rem;
    opacity: 0.85;
  }
  .findings {
    list-style: none;
    margin: 14px 0 0;
    padding: 0;
    display: grid;
    gap: 8px;
  }
  .findings li {
    display: grid;
    grid-template-columns: 70px 1fr;
    gap: 10px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-left-width: 4px;
    border-radius: 10px;
  }
  .findings li.high {
    border-left-color: var(--danger);
  }
  .findings li.medium {
    border-left-color: var(--warn);
  }
  .findings li.info {
    border-left-color: var(--muted);
  }
  .sev {
    font-size: 0.7rem;
    font-weight: 800;
    letter-spacing: 0.04em;
    padding-top: 2px;
  }
  .high .sev {
    color: var(--danger);
  }
  .medium .sev {
    color: var(--warn-fg);
  }
  .info .sev {
    color: var(--muted);
  }
  .loc {
    display: block;
    font-size: 0.8rem;
    color: var(--muted);
  }
  code {
    display: block;
    margin-top: 4px;
    padding: 6px 8px;
    border-radius: 6px;
    background: var(--surface-2);
    font-size: 0.78rem;
    white-space: pre-wrap;
    word-break: break-all;
  }
  .ack {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-top: 14px;
    font-weight: 600;
  }
</style>
