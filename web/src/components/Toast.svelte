<script lang="ts">
  import { Alert, CheckCircle, Info, X } from '../icons';
  import { dismissToast, toasts } from '../lib/store';
</script>

<div class="pointer-events-none fixed bottom-4 left-4 right-4 z-[80] flex flex-col items-center gap-2 md:bottom-auto md:left-auto md:right-4 md:top-4 md:items-end" aria-live="polite">
  {#each $toasts as item (item.id)}
    <div class="pointer-events-auto flex w-full max-w-sm items-start gap-2 rounded-md border border-border-2 bg-panel-2 px-3 py-2 text-[13px] shadow-lg">
      <span class="mt-0.5 flex-none" class:text-danger={item.kind === 'error'} class:text-accent={item.kind === 'success'} class:text-muted={item.kind === 'info'}>
        {#if item.kind === 'error'}<Alert size={16} />{:else if item.kind === 'success'}<CheckCircle size={16} />{:else}<Info size={16} />{/if}
      </span>
      <span class="min-w-0 flex-1 overflow-wrap-anywhere">{item.message}</span>
      <button class="btn btn-ghost btn-sm" style="min-height:28px;width:28px" aria-label="Dismiss" on:click={() => dismissToast(item.id)}><X size={16} /></button>
    </div>
  {/each}
</div>
