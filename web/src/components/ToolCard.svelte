<script lang="ts">
  import { Terminal, ChevronDown, ChevronUp, Check, X, Copy, Loader, Clock } from '../icons';
  import { copyText } from '../lib/clipboard';
  import { t } from '../lib/i18n';
  import { toast } from '../lib/store';
  import type { ViewItem } from '../lib/view';

  export let event: ViewItem;

  let open = false;
  $: name = event.name || 'tool';
  $: argText = typeof event.args === 'string' ? event.args : JSON.stringify(event.args ?? {}, null, 2);
  $: output = typeof event.output === 'string' ? event.output : JSON.stringify(event.output ?? '', null, 2);
  $: done = event.status === 'done' || event.status === 'error';
  $: failed = event.status === 'error' || event.is_error;

  async function copyOutput() {
    if (await copyText(output)) {
      toast($t('copied'), 'success');
    } else toast($t('error'), 'error');
  }
</script>

<div class="card overflow-hidden">
  <button
    class="flex min-h-[44px] w-full items-center gap-2 px-3 py-2 text-left hover:bg-panel-2"
    aria-expanded={open}
    on:click={() => (open = !open)}
  >
    <span class="flex-none {failed ? 'text-danger' : 'text-accent'}">
      {#if !done}<Loader size={16} class="spin" />{:else if failed}<X size={16} />{:else}<Check size={16} />{/if}
    </span>
    <Terminal size={16} class="flex-none text-muted" />
    <span class="min-w-0 flex-1 truncate mono text-[12px]">{name}</span>
    {#if event.elapsed_ms !== undefined}
      <span class="chip tiny flex-none"><Clock size={16} /> {event.elapsed_ms}ms</span>
    {/if}
    <span class="flex-none text-muted">{#if open}<ChevronUp size={16} />{:else}<ChevronDown size={16} />{/if}</span>
  </button>
  {#if open}
    <div class="border-t border-border px-3 py-2">
      <div class="tiny muted mb-1">{$t('input')}</div>
      <pre class="mono max-h-40 overflow-auto whitespace-pre-wrap rounded-sm bg-bg p-2 text-[11px]">{argText}</pre>
      {#if done}
        <div class="mb-1 mt-3 flex items-center justify-between">
          <span class="tiny muted">{$t('output')}</span>
          <button class="btn btn-ghost btn-sm" style="min-height:30px" aria-label={$t('copy')} on:click={copyOutput}><Copy size={16} /></button>
        </div>
        <pre class="mono max-h-64 overflow-auto whitespace-pre-wrap rounded-sm bg-bg p-2 text-[11px]" class:text-danger={failed}>{output}</pre>
      {/if}
    </div>
  {/if}
</div>
