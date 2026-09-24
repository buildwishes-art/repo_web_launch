<script lang="ts">
  import { tick } from 'svelte';
  import { api } from '../lib/api';
  import { dialogs } from '../lib/dialogs.svelte';
  import { toast } from '../lib/store.svelte';
  import Icon from './Icon.svelte';
  import Modal from './Modal.svelte';

  // Log stdout/stderr (polling 1 detik). Tetap tersedia setelah crash untuk melihat penyebabnya.
  let lines = $state<string[]>([]);
  let follow = $state(true);
  let box: HTMLDivElement | undefined = $state();

  $effect(() => {
    const repo = dialogs.logs;
    if (!repo) return;
    lines = [];
    let alive = true;
    const load = async () => {
      const next = await api.getLogs(repo.id).catch(() => null);
      if (!alive || !next) return;
      if (next.length !== lines.length || next.at(-1) !== lines.at(-1)) {
        lines = next;
        if (follow) {
          await tick();
          box?.scrollTo({ top: box.scrollHeight });
        }
      }
    };
    load();
    const timer = setInterval(load, 1000);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  });

  async function copyAll() {
    await navigator.clipboard.writeText(lines.join('\n'));
    toast('Log disalin', 'success', 2500);
  }
</script>

<Modal open={!!dialogs.logs} wide title="Log · {dialogs.logs?.name ?? ''}" onclose={() => (dialogs.logs = null)}>
  <div class="toolbar">
    <span class="muted">{lines.length} baris</span>
    <label><input type="checkbox" bind:checked={follow} /> Auto-scroll</label>
    <button class="btn ghost small" onclick={copyAll}><Icon name="copy" size={15} />Salin semua</button>
  </div>
  <div class="log" bind:this={box}>
    {#if lines.length === 0}
      <p class="muted">Belum ada output.</p>
    {:else}
      {#each lines as line, i (i)}
        <div class:meta={line.startsWith('[repolaunch]') || line.startsWith('$ ')}>{line}</div>
      {/each}
    {/if}
  </div>
</Modal>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 8px;
  }
  .toolbar span {
    flex: 1;
  }
  .log {
    height: min(60vh, 520px);
    overflow: auto;
    padding: 12px;
    border-radius: 10px;
    background: var(--code-bg);
    color: var(--code-fg);
    font: 12.5px/1.45 'Cascadia Mono', Consolas, monospace;
    white-space: pre-wrap;
    word-break: break-all;
    user-select: text;
  }
  .meta {
    color: var(--code-accent);
    font-weight: 600;
  }
</style>
