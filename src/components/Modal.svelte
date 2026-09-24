<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';

  let {
    open,
    title,
    onclose,
    wide = false,
    tone = 'default',
    children,
    actions,
  }: {
    open: boolean;
    title: string;
    onclose: () => void;
    wide?: boolean;
    tone?: 'default' | 'danger';
    children: Snippet;
    actions?: Snippet;
  } = $props();

  let el: HTMLDialogElement | undefined = $state();

  // Sinkronkan prop `open` dengan <dialog> native (fokus trap & Esc gratis dari browser).
  $effect(() => {
    if (!el) return;
    if (open && !el.open) el.showModal();
    else if (!open && el.open) el.close();
  });
</script>

<dialog
  bind:this={el}
  class:wide
  class:danger={tone === 'danger'}
  oncancel={(e) => {
    e.preventDefault();
    onclose();
  }}
>
  {#if open}
    <header>
      <h2>{title}</h2>
      <button class="icon-btn" onclick={onclose} aria-label="Tutup"><Icon name="x" /></button>
    </header>
    <div class="body">{@render children()}</div>
    {#if actions}
      <footer>{@render actions()}</footer>
    {/if}
  {/if}
</dialog>

<style>
  dialog {
    width: min(560px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 16px;
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow-lg);
  }
  dialog.wide {
    width: min(900px, calc(100vw - 32px));
  }
  dialog.danger {
    border-top: 4px solid var(--danger);
  }
  dialog::backdrop {
    background: rgb(0 0 0 / 0.45);
    backdrop-filter: blur(2px);
  }
  dialog[open] {
    display: flex;
    flex-direction: column;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px 8px;
  }
  h2 {
    margin: 0;
    font-size: 1.1rem;
  }
  .body {
    padding: 8px 20px 16px;
    overflow: auto;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 20px 16px;
    border-top: 1px solid var(--border);
  }
</style>
