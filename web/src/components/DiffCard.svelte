<script lang="ts">
  import { FileDiff, ChevronDown, ChevronUp, Check, X, Undo, External } from '../icons';
  import { parseDiff } from '../lib/diff';
  import { api } from '../lib/api';
  import { t } from '../lib/i18n';
  import { toast } from '../lib/store';
  import type { ViewItem } from '../lib/view';

  export let event: ViewItem;
  export let onOpen: () => void = () => {};

  let open = false;
  let busyAction = false;

  $: files = parseDiff(event.diff || event.patch || '');
  $: added = files.reduce((n, f) => n + f.added, 0);
  $: removed = files.reduce((n, f) => n + f.removed, 0);

  async function action(kind: 'apply' | 'reject' | 'undo') {
    busyAction = true;
    try {
      await api.diffAction(kind);
      toast(`${$t(kind)} · OK`, 'success');
    } catch (err) {
      toast(err instanceof Error ? err.message : String(err), 'error');
    } finally {
      busyAction = false;
    }
  }
</script>

<div class="card overflow-hidden">
  <button class="flex min-h-[44px] w-full items-center gap-2 px-3 py-2 text-left hover:bg-panel-2" on:click={() => (open = !open)} aria-expanded={open}>
    <FileDiff size={16} class="text-accent" />
    <span class="min-w-0 flex-1 truncate mono text-[12px]">{files[0]?.path || event.path || 'changes'}</span>
    <span class="chip text-add-fg">+{added}</span>
    <span class="chip text-del-fg">−{removed}</span>
    {#if open}<ChevronUp size={16} />{:else}<ChevronDown size={16} />{/if}
  </button>
  {#if open}
    <div class="border-t border-border p-2">
      <pre class="mono max-h-72 overflow-auto whitespace-pre-wrap text-[11px]">{(event.diff || '').slice(0, 4000)}</pre>
    </div>
  {/if}
  <div class="flex flex-wrap gap-2 border-t border-border px-3 py-2">
    <button class="btn btn-sm" disabled={busyAction} on:click={() => action('apply')}><Check size={16} /> {$t('apply')}</button>
    <button class="btn btn-sm" disabled={busyAction} on:click={() => action('reject')}><X size={16} /> {$t('reject')}</button>
    <button class="btn btn-sm" disabled={busyAction} on:click={() => action('undo')}><Undo size={16} /> {$t('undo')}</button>
    <button class="btn btn-ghost btn-sm" on:click={onOpen}><External size={16} /> {$t('diff')}</button>
  </div>
</div>
