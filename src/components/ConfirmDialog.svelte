<script lang="ts">
  import { closeConfirm, dialogs } from '../lib/dialogs.svelte';
  import Modal from './Modal.svelte';

  const c = $derived(dialogs.confirm);
</script>

<Modal open={!!c} title={c?.title ?? ''} tone={c?.danger ? 'danger' : 'default'} onclose={() => closeConfirm(false)}>
  <p>{c?.message}</p>
  {#snippet actions()}
    <button class="btn ghost" onclick={() => closeConfirm(false)}>Batal</button>
    <button class={c?.danger ? 'btn danger-solid' : 'btn primary'} onclick={() => closeConfirm(true)}>
      {c?.confirmLabel}
    </button>
  {/snippet}
</Modal>
