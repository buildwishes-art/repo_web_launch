<script lang="ts">
  import Icon, { type IconName } from './Icon.svelte';

  // Bar horizontal dengan penanda ambang batas; merah jika melewati ambang.
  let {
    icon,
    label,
    value,
    ratio,
    threshold,
  }: { icon: IconName; label: string; value: string; ratio: number; threshold: number } = $props();

  const over = $derived(ratio > threshold);
  const pct = $derived(Math.min(Math.max(ratio, 0), 1) * 100);
</script>

<div class="meter" class:over>
  <Icon name={icon} size={15} />
  <span class="label">{label}</span>
  <div class="track">
    <div class="fill" style:width="{pct}%"></div>
    <div class="mark" style:left="{Math.min(threshold, 1) * 100}%" title="Ambang batas"></div>
  </div>
  <span class="value">{value}</span>
</div>

<style>
  .meter {
    display: grid;
    grid-template-columns: 16px 36px 1fr 72px;
    align-items: center;
    gap: 8px;
    color: var(--muted);
    font-size: 0.82rem;
  }
  .track {
    position: relative;
    height: 8px;
    border-radius: 999px;
    background: var(--surface-2);
  }
  .fill {
    height: 100%;
    border-radius: 999px;
    background: var(--primary);
    transition: width 0.4s ease;
  }
  .mark {
    position: absolute;
    top: -3px;
    width: 2px;
    height: 14px;
    background: var(--muted);
    opacity: 0.6;
  }
  .value {
    text-align: right;
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .over .fill {
    background: var(--danger);
  }
  .over .value {
    color: var(--danger);
    font-weight: 600;
  }
</style>
