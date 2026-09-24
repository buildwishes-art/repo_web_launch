<script lang="ts">
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import { api, errorText } from '../lib/api';
  import { dialogs, showScanReport } from '../lib/dialogs.svelte';
  import { refresh, toast } from '../lib/store.svelte';
  import Icon from './Icon.svelte';
  import Modal from './Modal.svelte';

  // Form tambah repo: link Git (https) atau arsip lokal. Setelah clone/ekstrak, Rust menolak
  // repo non-web dan memindai keamanan di karantina sebelum repo masuk workspace.
  let source = $state<'git' | 'archive'>('git');
  let url = $state('');
  let name = $state('');
  let archivePath = $state<string | null>(null);
  let urlError = $state<string | null>(null);
  let working = $state(false);

  const archiveName = $derived(archivePath?.split(/[\\/]/).pop() ?? null);

  function reset() {
    url = '';
    name = '';
    archivePath = null;
    urlError = null;
    source = 'git';
  }

  function close() {
    if (working) return; // jangan tutup saat clone/scan berjalan
    dialogs.addRepo = false;
    reset();
  }

  async function validateUrl() {
    urlError = url.trim() ? await api.validateGitUrl(url) : null;
  }

  async function pickArchive() {
    const selected = await openFile({
      multiple: false,
      directory: false,
      filters: [{ name: 'Arsip repo', extensions: ['zip', 'gz', 'tgz'] }],
    });
    if (typeof selected === 'string') archivePath = selected;
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (source === 'git') {
      await validateUrl();
      if (urlError || !url.trim()) return;
    } else if (!archivePath) {
      toast('Pilih file arsip terlebih dahulu', 'error');
      return;
    }

    working = true;
    try {
      const n = name.trim() || null;
      const info =
        source === 'git' ? await api.addRepoFromGit(url.trim(), n) : await api.addRepoFromArchive(archivePath!, n);
      await refresh();
      working = false;
      close();

      if (info.scanVerdict === 'suspicious') {
        // Scan di karantina menemukan hal mencurigakan → tampilkan note + temuan.
        const choice = await showScanReport({ repoId: info.id, repoName: info.name, offerDelete: true });
        if (choice === 'delete') {
          await api.deleteRepo(info.id);
          await refresh();
          toast(`"${info.name}" dihapus`, 'success');
        }
      } else {
        toast(`"${info.name}" terinstal (${info.stack}) · scan keamanan: aman`, 'success');
      }
    } catch (err) {
      toast(errorText(err), 'error', 9000);
    } finally {
      working = false;
    }
  }
</script>

<Modal open={dialogs.addRepo} title="Tambah repo web" onclose={close}>
  <form id="add-repo" onsubmit={submit}>
    <div class="segmented" role="tablist">
      <button type="button" class:active={source === 'git'} disabled={working} onclick={() => (source = 'git')}>
        <Icon name="link" size={16} />Link Git
      </button>
      <button type="button" class:active={source === 'archive'} disabled={working} onclick={() => (source = 'archive')}>
        <Icon name="archive" size={16} />Arsip .zip / .tar.gz
      </button>
    </div>

    {#if source === 'git'}
      <label class="field">
        <span>URL repositori (https)</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input
          type="url"
          placeholder="https://github.com/owner/repo"
          bind:value={url}
          onblur={validateUrl}
          oninput={() => (urlError = null)}
          disabled={working}
          autocomplete="off"
          spellcheck="false"
          autofocus
        />
        {#if urlError}<small class="error">{urlError}</small>{/if}
      </label>
    {:else}
      <button type="button" class="btn outline file" onclick={pickArchive} disabled={working}>
        <Icon name="download" size={16} />
        {archiveName ?? 'Pilih file arsip (maks. 512 MB)'}
      </button>
    {/if}

    <label class="field">
      <span>Nama (opsional)</span>
      <input bind:value={name} disabled={working} placeholder="otomatis dari nama repo" />
      <small>Hanya huruf, angka, titik, - dan _. Karakter lain diganti “-”.</small>
    </label>

    <p class="hint">
      <Icon name="shield" size={14} />
      Repo dipindai keamanannya di folder karantina sebelum boleh dijalankan. Hanya proyek web
      (Node.js, HTML statis, PHP/Laravel, Django/FastAPI/Flask) yang diterima.
    </p>
  </form>

  {#snippet actions()}
    <button class="btn ghost" onclick={close} disabled={working}>Batal</button>
    <button class="btn primary" type="submit" form="add-repo" disabled={working}>
      {#if working}
        <span class="spinner small"></span>{source === 'git' ? 'Meng-clone & memindai…' : 'Mengekstrak & memindai…'}
      {:else}
        <Icon name="download" size={16} />Install & scan
      {/if}
    </button>
  {/snippet}
</Modal>

<style>
  form {
    display: grid;
    gap: 14px;
  }
  .segmented {
    display: grid;
    grid-template-columns: 1fr 1fr;
    padding: 4px;
    border-radius: 12px;
    background: var(--surface-2);
  }
  .segmented button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 8px;
    border: none;
    border-radius: 9px;
    background: none;
    color: var(--muted);
    font-weight: 600;
    cursor: pointer;
  }
  .segmented button.active {
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow-sm);
  }
  .file {
    justify-content: flex-start;
    min-height: 48px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hint {
    display: flex;
    gap: 8px;
    margin: 0;
    font-size: 0.8rem;
    color: var(--muted);
  }
</style>
