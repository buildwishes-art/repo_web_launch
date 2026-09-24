<script lang="ts">
  import { api, errorText } from '../lib/api';
  import { confirm, dialogs, showScanReport } from '../lib/dialogs.svelte';
  import { app, formatMem, isActive, refresh, statusLabel, toast, withBusy } from '../lib/store.svelte';
  import { SUSPICIOUS_NOTE, type RepoInfo } from '../lib/types';
  import Icon from './Icon.svelte';
  import Meter from './Meter.svelte';

  let { repo }: { repo: RepoInfo } = $props();

  const metric = $derived(app.metrics[repo.id]);
  const busy = $derived(!!app.busy[repo.id]);
  const active = $derived(isActive(repo.status));
  const pending = $derived(repo.status === 'installing' || repo.status === 'starting');
  const suspicious = $derived(repo.scanVerdict === 'suspicious');
  const cpu = $derived(metric?.cpu ?? repo.cpuPercent);
  const mem = $derived(metric?.memMb ?? repo.memoryMb);
  const memLimit = $derived(app.settings?.memoryThresholdMb ?? 1024);
  let menuOpen = $state(false);

  async function start() {
    // Repo mencurigakan: tampilkan temuan & minta konfirmasi eksplisit (Rust juga menolak tanpa flag).
    let allowSuspicious = false;
    if (suspicious) {
      const choice = await showScanReport({ repoId: repo.id, repoName: repo.name, offerRun: true });
      if (choice !== 'run') return;
      allowSuspicious = true;
    }
    try {
      const info = await withBusy(repo.id, () => api.startRepo(repo.id, allowSuspicious));
      if (info.notice) toast(info.notice);
      await refresh();
    } catch (e) {
      toast(errorText(e), 'error', 8000);
    }
  }

  async function stop() {
    try {
      await withBusy(repo.id, () => api.stopRepo(repo.id));
      await refresh();
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }

  async function openUrl() {
    try {
      await api.openRepoUrl(repo.id);
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }

  async function copyUrl() {
    if (!repo.url) return;
    await navigator.clipboard.writeText(repo.url);
    toast('Link disalin', 'success', 2500);
  }

  async function rescan() {
    menuOpen = false;
    toast(`Memindai ${repo.name}…`, 'info', 3000);
    try {
      const report = await withBusy(repo.id, () => api.rescanRepo(repo.id));
      await refresh();
      await showScanReport({ repoId: repo.id, repoName: repo.name, report });
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }

  async function cleanCache() {
    menuOpen = false;
    try {
      const mb = await api.cleanCache(repo.id);
      toast(`Cache dibersihkan: ${mb.toFixed(1)} MB dibebaskan`, 'success');
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }

  async function remove() {
    menuOpen = false;
    const ok = await confirm({
      title: `Hapus "${repo.name}"?`,
      message:
        'Repo akan dihentikan dan SELURUH filenya (termasuk node_modules, vendor, .venv, dan cache) dihapus permanen dari disk. Tindakan ini tidak bisa dibatalkan.',
      confirmLabel: 'Hapus',
      danger: true,
    });
    if (!ok) return;
    try {
      await withBusy(repo.id, () => api.deleteRepo(repo.id));
      toast(`"${repo.name}" dihapus`, 'success');
      await refresh();
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }
</script>

<svelte:window onclick={() => (menuOpen = false)} />

<article class="card" class:suspicious>
  <div class="head">
    <span class="dot {repo.status}" title={statusLabel[repo.status]}></span>
    <div class="title">
      <h3>{repo.name}</h3>
      <p>{repo.stack} · {repo.sourceKind === 'git' ? 'Git' : 'Arsip'} · {statusLabel[repo.status]}</p>
    </div>

    <button
      class="badge scan-{repo.scanVerdict}"
      title="Hasil scan keamanan — klik untuk detail"
      onclick={() => showScanReport({ repoId: repo.id, repoName: repo.name })}
    >
      <Icon
        name={repo.scanVerdict === 'clean' ? 'shieldCheck' : suspicious ? 'shieldAlert' : 'shield'}
        size={14}
      />
      {repo.scanVerdict === 'clean' ? 'Aman' : suspicious ? 'Tidak normal' : 'Belum dipindai'}
    </button>

    {#if repo.port !== null}
      <span
        class="badge port"
        class:relocated={!!repo.notice && active}
        class:live={active}
        title={repo.notice && active ? 'Port dialihkan otomatis karena konflik' : 'Port'}
      >
        <Icon name={repo.notice && active ? 'route' : 'lan'} size={14} />:{repo.port}
      </span>
    {/if}

    <div class="menu-wrap">
      <button
        class="icon-btn"
        aria-label="Opsi"
        onclick={(e) => {
          e.stopPropagation();
          menuOpen = !menuOpen;
        }}><Icon name="more" /></button
      >
      {#if menuOpen}
        <div class="menu" role="menu">
          <button onclick={() => ((menuOpen = false), (dialogs.logs = repo))}><Icon name="terminal" size={16} />Lihat log</button>
          <button onclick={() => ((menuOpen = false), showScanReport({ repoId: repo.id, repoName: repo.name }))}
            ><Icon name="shield" size={16} />Hasil scan keamanan</button
          >
          <button onclick={rescan} disabled={busy}><Icon name="refresh" size={16} />Scan ulang</button>
          <button onclick={cleanCache} disabled={active}><Icon name="sparkles" size={16} />Bersihkan cache</button>
          <hr />
          <button class="danger" onclick={remove} disabled={busy}><Icon name="trash" size={16} />Hapus (wipe data)</button>
        </div>
      {/if}
    </div>
  </div>

  {#if suspicious}
    <div class="notice error">
      <Icon name="alert" size={16} />
      <span><strong>Note: {SUSPICIOUS_NOTE}.</strong> {repo.scanSummary ?? ''} — klik badge “Tidak normal” untuk detail.</span>
    </div>
  {/if}

  {#if pending}
    <div class="progress"><div></div></div>
    <p class="phase">{repo.phase ?? statusLabel[repo.status]}</p>
  {/if}

  {#if repo.url && repo.status === 'running'}
    <div class="link-row">
      <button class="link" onclick={openUrl}><Icon name="external" size={15} />{repo.url}</button>
      <button class="icon-btn" aria-label="Salin link" onclick={copyUrl}><Icon name="copy" size={16} /></button>
    </div>
  {/if}

  {#if repo.notice}
    <div class="notice" class:error={repo.status === 'crashed'}>
      <Icon name={repo.status === 'crashed' ? 'alert' : 'info'} size={16} />
      <span>{repo.notice}</span>
    </div>
  {/if}

  {#if active}
    <div class="meters">
      <Meter
        icon="cpu"
        label="CPU"
        value="{cpu.toFixed(0)}%"
        ratio={cpu / 100}
        threshold={(app.settings?.cpuThresholdPercent ?? 80) / 100}
      />
      <!-- RAM: ambang ditaruh di 80% lebar bar agar ada ruang visual di atasnya -->
      <Meter icon="memory" label="RAM" value={formatMem(mem)} ratio={(mem / memLimit) * 0.8} threshold={0.8} />
    </div>
  {/if}

  <div class="actions">
    {#if busy}
      <span class="spinner" aria-label="Memproses"></span>
    {:else if active}
      <button class="btn tonal" onclick={stop}><Icon name="stop" size={16} />Stop</button>
    {:else}
      <button class="btn primary" onclick={start}><Icon name="play" size={16} />Start</button>
    {/if}
  </div>
</article>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: 16px;
    background: var(--surface);
  }
  .card.suspicious {
    border-color: color-mix(in srgb, var(--danger) 50%, var(--border));
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .title {
    flex: 1;
    min-width: 0;
  }
  h3 {
    margin: 0;
    font-size: 1rem;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .title p {
    margin: 2px 0 0;
    font-size: 0.8rem;
    color: var(--muted);
  }
  .dot {
    width: 11px;
    height: 11px;
    border-radius: 50%;
    flex: none;
    background: var(--muted);
  }
  .dot.running {
    background: var(--ok);
    box-shadow: 0 0 0 4px color-mix(in srgb, var(--ok) 20%, transparent);
  }
  .dot.installing,
  .dot.starting {
    background: var(--warn);
    animation: pulse 1.2s infinite;
  }
  .dot.crashed {
    background: var(--danger);
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  .badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px 10px;
    border: none;
    border-radius: 999px;
    font-size: 0.78rem;
    font-weight: 600;
    white-space: nowrap;
    background: var(--surface-2);
    color: var(--muted);
  }
  button.badge {
    cursor: pointer;
  }
  .scan-clean {
    background: var(--ok-bg);
    color: var(--ok-fg);
  }
  .scan-suspicious {
    background: var(--danger-bg);
    color: var(--danger-fg);
  }
  .port {
    font-variant-numeric: tabular-nums;
  }
  .port.live {
    background: var(--ok-bg);
    color: var(--ok-fg);
  }
  .port.relocated {
    background: var(--warn-bg);
    color: var(--warn-fg);
  }

  .menu-wrap {
    position: relative;
  }
  .menu {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    z-index: 10;
    min-width: 210px;
    padding: 6px;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--surface);
    box-shadow: var(--shadow-lg);
  }
  .menu button {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border: none;
    border-radius: 8px;
    background: none;
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }
  .menu button:hover:not(:disabled) {
    background: var(--surface-2);
  }
  .menu button:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .menu .danger {
    color: var(--danger);
  }
  .menu hr {
    margin: 4px 0;
    border: none;
    border-top: 1px solid var(--border);
  }

  .notice {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    padding: 10px 12px;
    border-radius: 10px;
    font-size: 0.85rem;
    background: var(--info-bg);
    color: var(--info-fg);
  }
  .notice.error {
    background: var(--danger-bg);
    color: var(--danger-fg);
  }

  .progress {
    height: 4px;
    border-radius: 999px;
    overflow: hidden;
    background: var(--surface-2);
  }
  .progress div {
    width: 35%;
    height: 100%;
    background: var(--primary);
    animation: slide 1.1s ease-in-out infinite;
  }
  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(300%);
    }
  }
  .phase {
    margin: -6px 0 0;
    font-size: 0.8rem;
    color: var(--muted);
  }

  .link-row {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .link {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    padding: 4px 2px;
    border: none;
    background: none;
    color: var(--primary);
    font-weight: 600;
    text-decoration: underline;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meters {
    display: grid;
    gap: 6px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
</style>
