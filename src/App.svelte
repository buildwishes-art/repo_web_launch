<script lang="ts">
  import { onMount } from 'svelte';
  import AddRepoDialog from './components/AddRepoDialog.svelte';
  import ConfirmDialog from './components/ConfirmDialog.svelte';
  import Icon from './components/Icon.svelte';
  import LogViewer from './components/LogViewer.svelte';
  import RepoCard from './components/RepoCard.svelte';
  import ScanReportDialog from './components/ScanReportDialog.svelte';
  import SettingsView from './components/SettingsView.svelte';
  import Toasts from './components/Toasts.svelte';
  import { errorText } from './lib/api';
  import { dialogs } from './lib/dialogs.svelte';
  import { app, formatMem, initStore, isActive, refresh } from './lib/store.svelte';

  let initError = $state<string | null>(null);

  onMount(() => {
    initStore().catch((e) => (initError = errorText(e)));
  });

  // Metrik live (event tiap 2 dtk) dengan fallback ke snapshot terakhir dari daftar repo.
  const activeRepos = $derived(app.repos.filter((r) => isActive(r.status)));
  const totalCpu = $derived(activeRepos.reduce((a, r) => a + (app.metrics[r.id]?.cpu ?? r.cpuPercent), 0));
  const totalMem = $derived(activeRepos.reduce((a, r) => a + (app.metrics[r.id]?.memMb ?? r.memoryMb), 0));
</script>

<div class="shell">
  <header class="topbar">
    <div class="brand">
      <span class="logo"><Icon name="rocket" size={20} /></span>
      <div>
        <h1>RepoLaunch</h1>
        <p>Launcher & monitor repositori web lokal</p>
      </div>
    </div>
    <div class="chips">
      <span class="chip"><Icon name="play" size={14} />{activeRepos.length} aktif · {app.repos.length} terinstal</span>
      <span class="chip"><Icon name="cpu" size={14} />CPU {totalCpu.toFixed(0)}%</span>
      <span class="chip"><Icon name="memory" size={14} />RAM {formatMem(totalMem)}</span>
    </div>
    <div class="top-actions">
      {#if app.view === 'repos'}
        <button class="icon-btn" title="Muat ulang" aria-label="Muat ulang" onclick={refresh}><Icon name="refresh" /></button>
        <button class="btn outline" onclick={() => (app.view = 'settings')} disabled={!app.settings}>
          <Icon name="sliders" size={16} />Pengaturan
        </button>
        <button class="btn primary" onclick={() => (dialogs.addRepo = true)}><Icon name="plus" size={16} />Tambah repo</button>
      {:else}
        <button class="btn outline" onclick={() => (app.view = 'repos')}>← Daftar repo</button>
      {/if}
    </div>
  </header>

  <main>
    {#if initError}
      <div class="state error">Gagal menghubungi core: {initError}</div>
    {:else if app.view === 'settings' && app.settings}
      <SettingsView />
    {:else if !app.loaded}
      <div class="state"><span class="spinner"></span></div>
    {:else if app.loadError}
      <div class="state error">Gagal memuat repo: {app.loadError}</div>
    {:else if app.repos.length === 0}
      <div class="empty">
        <span class="logo big"><Icon name="rocket" size={40} /></span>
        <h2>Belum ada repo</h2>
        <p>
          Tempel link Git (https) atau unggah arsip .zip / .tar.gz proyek web.<br />
          RepoLaunch memindai keamanannya, mendeteksi cara menjalankannya, dan memilih port yang aman.
        </p>
        <button class="btn primary" onclick={() => (dialogs.addRepo = true)}><Icon name="plus" size={16} />Tambah repo pertama</button>
      </div>
    {:else}
      <div class="grid">
        {#each app.repos as repo (repo.id)}
          <RepoCard {repo} />
        {/each}
      </div>
    {/if}
  </main>
</div>

<AddRepoDialog />
<ScanReportDialog />
<ConfirmDialog />
<LogViewer />
<Toasts />

<style>
  .shell {
    max-width: 1240px;
    margin: 0 auto;
    padding: 0 20px;
  }
  .topbar {
    position: sticky;
    top: 0;
    z-index: 5;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px 20px;
    padding: 16px 0;
    background: var(--bg);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .brand h1 {
    margin: 0;
    font-size: 1.15rem;
  }
  .brand p {
    margin: 0;
    font-size: 0.78rem;
    color: var(--muted);
  }
  .logo {
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    border-radius: 11px;
    color: #fff;
    background: linear-gradient(135deg, #4f46e5, #0ea5e9);
  }
  .logo.big {
    width: 72px;
    height: 72px;
    border-radius: 20px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    flex: 1;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 5px 10px;
    border: 1px solid var(--border);
    border-radius: 999px;
    font-size: 0.8rem;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    color: var(--muted);
  }
  .top-actions {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  main {
    padding-bottom: 40px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(440px, 1fr));
    gap: 14px;
  }
  .state {
    display: grid;
    place-items: center;
    min-height: 40vh;
  }
  .empty {
    display: grid;
    justify-items: center;
    gap: 10px;
    margin-top: 12vh;
    text-align: center;
  }
  .empty h2 {
    margin: 8px 0 0;
  }
  .empty p {
    margin: 0 0 12px;
    color: var(--muted);
    line-height: 1.5;
  }
</style>
