<script lang="ts">
  import { toasts } from '../lib/toast.svelte'
</script>

<div class="toasts" role="status" aria-live="polite">
  {#each toasts.items as t (t.id)}
    <div class="toast card {t.kind}">
      <span>{t.message}</span>
      {#if t.action}
        <button
          class="primary"
          onclick={() => {
            t.action?.run()
            toasts.dismiss(t.id)
          }}>{t.action.label}</button
        >
      {/if}
      <button class="ghost icon" aria-label="Dismiss" onclick={() => toasts.dismiss(t.id)}>✕</button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 16px;
    bottom: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 100;
    max-width: min(440px, calc(100vw - 32px));
  }
  .toast {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 10px 12px;
  }
  .toast span {
    flex: 1;
  }
  .toast.error {
    border-left: 4px solid var(--danger);
  }
  .toast.success {
    border-left: 4px solid var(--ok);
  }
  .toast.info {
    border-left: 4px solid var(--accent);
  }
</style>
