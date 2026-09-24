<script lang="ts">
  import { dismissToast, toasts } from '../lib/store.svelte';
  import Icon from './Icon.svelte';
</script>

<div class="toasts" aria-live="polite">
  {#each toasts.items as t (t.id)}
    <div class="toast {t.kind}">
      <Icon name={t.kind === 'error' ? 'alert' : t.kind === 'success' ? 'shieldCheck' : 'info'} size={16} />
      <span>{t.text}</span>
      <button class="icon-btn" aria-label="Tutup" onclick={() => dismissToast(t.id)}><Icon name="x" size={14} /></button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    left: 50%;
    bottom: 24px;
    z-index: 100;
    display: grid;
    gap: 8px;
    width: min(560px, calc(100vw - 32px));
    transform: translateX(-50%);
  }
  .toast {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 10px 10px 14px;
    border-radius: 12px;
    background: var(--inverse-bg);
    color: var(--inverse-fg);
    box-shadow: var(--shadow-lg);
    font-size: 0.88rem;
    animation: rise 0.18s ease-out;
  }
  .toast span {
    flex: 1;
    padding-top: 1px;
  }
  .toast.error {
    background: var(--danger-bg);
    color: var(--danger-fg);
  }
  .toast .icon-btn {
    color: inherit;
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }
</style>
