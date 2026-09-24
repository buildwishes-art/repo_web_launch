<script lang="ts">
  import { api, errorText } from '../lib/api';
  import { app, formatMem, toast } from '../lib/store.svelte';
  import type { AppSettings } from '../lib/types';
  import Icon from './Icon.svelte';

  // Draft lokal; disimpan ke Rust (yang meng-clamp nilai ke rentang aman) saat "Simpan".
  let draft = $state<AppSettings>($state.snapshot(app.settings!) as AppSettings);
  let saving = $state(false);

  const coreOptions = $derived([
    [0, `Semua (${app.cpuCount})`] as const,
    ...Array.from({ length: Math.max(app.cpuCount - 1, 0) }, (_, i) => [i + 1, `${i + 1} core`] as const),
  ]);

  async function save() {
    saving = true;
    try {
      app.settings = await api.updateSettings($state.snapshot(draft));
      draft = $state.snapshot(app.settings) as AppSettings;
      toast('Pengaturan disimpan. Batas CPU/RAM berlaku untuk repo yang di-start berikutnya.', 'success');
    } catch (e) {
      toast(errorText(e), 'error');
    } finally {
      saving = false;
    }
  }

  async function cleanAll() {
    try {
      const mb = await api.cleanCache(null);
      toast(`Cache dibersihkan: ${mb.toFixed(1)} MB dibebaskan`, 'success');
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }
</script>

<div class="settings">
  <section>
    <h3><Icon name="tray" size={16} />System tray</h3>
    <label class="switch">
      <input type="checkbox" bind:checked={draft.minimizeToTray} />
      <span
        ><strong>Minimize ke tray</strong><small
          >Nonaktif (default): minimize biasa, RepoLaunch tetap terlihat di taskbar. Aktif: minimize menyembunyikan
          jendela dari taskbar ke system tray.</small
        ></span
      >
    </label>
    <label class="switch">
      <input type="checkbox" bind:checked={draft.closeToTray} />
      <span
        ><strong>Tombol close (X) ke tray</strong><small
          >Repo tetap berjalan & dipantau di latar belakang. Keluar lewat menu klik-kanan ikon tray → “Keluar”.</small
        ></span
      >
    </label>
  </section>

  <section>
    <h3><Icon name="alert" size={16} />Peringatan resource tinggi</h3>
    <label class="range">
      <span>Ambang CPU <b>{draft.cpuThresholdPercent.toFixed(0)}%</b></span>
      <input type="range" min="10" max="100" step="5" bind:value={draft.cpuThresholdPercent} />
    </label>
    <label class="range">
      <span>Ambang RAM <b>{formatMem(draft.memoryThresholdMb)}</b></span>
      <input type="range" min="256" max="8192" step="128" bind:value={draft.memoryThresholdMb} />
    </label>
    <label class="range">
      <span>Harus terlampaui selama <b>{draft.sustainSecs} detik</b></span>
      <input type="range" min="10" max="300" step="10" bind:value={draft.sustainSecs} />
    </label>
    <label class="select">
      <span>Kill otomatis jika notifikasi tidak dijawab</span>
      <select bind:value={draft.autoKillAfterSecs}>
        <option value={0}>Nonaktif</option>
        <option value={30}>30 detik</option>
        <option value={60}>1 menit</option>
        <option value={120}>2 menit</option>
        <option value={300}>5 menit</option>
      </select>
    </label>
    <label class="select">
      <span>Setelah menjawab “Tidak”, diamkan selama</span>
      <select bind:value={draft.snoozeSecs}>
        <option value={300}>5 menit</option>
        <option value={600}>10 menit</option>
        <option value={1800}>30 menit</option>
        <option value={3600}>1 jam</option>
      </select>
    </label>
  </section>

  <section>
    <h3><Icon name="refresh" size={16} />Repo idle</h3>
    <label class="select">
      <span>Hentikan repo idle setelah <small>(CPU &lt; 1% dan tanpa output log)</small></span>
      <select bind:value={draft.idleTimeoutMins}>
        <option value={0}>Nonaktif</option>
        <option value={10}>10 menit</option>
        <option value={30}>30 menit</option>
        <option value={60}>1 jam</option>
        <option value={180}>3 jam</option>
      </select>
    </label>
  </section>

  <section>
    <h3><Icon name="cpu" size={16} />Batas proses (Job Object)</h3>
    <label class="select">
      <span>Batasi ke jumlah core CPU</span>
      <select bind:value={draft.cpuCoresLimit}>
        {#each coreOptions as [v, label] (v)}<option value={v}>{label}</option>{/each}
      </select>
    </label>
    <label class="select">
      <span>Hard cap CPU</span>
      <select bind:value={draft.cpuRateLimitPercent}>
        <option value={0}>Tanpa batas</option>
        <option value={25}>25%</option>
        <option value={50}>50%</option>
        <option value={75}>75%</option>
      </select>
    </label>
    <label class="select">
      <span>Hard limit RAM <small>(proses yang melewati batas akan gagal alokasi)</small></span>
      <select bind:value={draft.memoryHardLimitMb}>
        <option value={0}>Tanpa batas</option>
        <option value={512}>512 MB</option>
        <option value={1024}>1 GB</option>
        <option value={2048}>2 GB</option>
        <option value={4096}>4 GB</option>
      </select>
    </label>
    <label class="switch">
      <input type="checkbox" bind:checked={draft.lowPriority} />
      <span><strong>Prioritas rendah</strong><small>RepoLaunch & Windows tetap responsif saat repo sedang build.</small></span>
    </label>
  </section>

  <section>
    <h3><Icon name="shield" size={16} />Keamanan</h3>
    <label class="switch">
      <input type="checkbox" bind:checked={draft.antivirusScan} />
      <span
        ><strong>Scan Windows Defender saat menambah repo</strong><small
          >Selain analisis statis, repo dipindai antivirus di folder karantina sebelum boleh dijalankan (maks. 3 menit).</small
        ></span
      >
    </label>
    <label class="switch">
      <input type="checkbox" bind:checked={draft.allowInstallScripts} />
      <span
        ><strong>Izinkan install scripts</strong><small
          >Nonaktif = <code>npm install --ignore-scripts</code> / <code>composer install --no-scripts</code>. Aktifkan hanya
          untuk repo tepercaya.</small
        ></span
      >
    </label>
  </section>

  <section>
    <h3><Icon name="sparkles" size={16} />Cache</h3>
    <label class="switch">
      <input type="checkbox" bind:checked={draft.cleanCacheOnStop} />
      <span><strong>Bersihkan cache saat repo dihentikan</strong></span>
    </label>
    <button class="btn outline" onclick={cleanAll}><Icon name="sparkles" size={16} />Bersihkan semua cache sekarang</button>
  </section>

  <div class="save-bar">
    <button class="btn ghost" onclick={() => (app.view = 'repos')}>Kembali</button>
    <button class="btn primary" onclick={save} disabled={saving}>
      {#if saving}<span class="spinner small"></span>{/if}Simpan pengaturan
    </button>
  </div>
</div>

<style>
  .settings {
    display: grid;
    gap: 16px;
    padding-bottom: 80px;
  }
  section {
    display: grid;
    gap: 12px;
    padding: 16px 18px;
    border: 1px solid var(--border);
    border-radius: 16px;
    background: var(--surface);
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 2px;
    font-size: 0.92rem;
    color: var(--primary);
  }
  .switch {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    cursor: pointer;
  }
  .switch input {
    margin-top: 3px;
    width: 18px;
    height: 18px;
    accent-color: var(--primary);
  }
  .switch span {
    display: grid;
    gap: 2px;
  }
  small {
    color: var(--muted);
    font-size: 0.8rem;
  }
  .range {
    display: grid;
    gap: 4px;
  }
  .range input {
    accent-color: var(--primary);
  }
  .select {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }
  .save-bar {
    position: sticky;
    bottom: 16px;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--surface);
    box-shadow: var(--shadow-lg);
  }
</style>
