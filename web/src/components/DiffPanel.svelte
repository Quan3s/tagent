<script lang="ts">
  import { Diff, ChevronDown, ChevronUp, Check, X, Undo, Refresh } from '../icons';
  import { api } from '../lib/api';
  import { diffText, toast } from '../lib/store';
  import { t } from '../lib/i18n';
  import { parseDiff } from '../lib/diff';

  let active = 0;
  let collapsed: Record<number, boolean> = {};
  let busy = false;

  $: files = parseDiff($diffText);
  $: file = files[Math.min(active, Math.max(0, files.length - 1))];

  async function reload() {
    try {
      const result = await api.diff();
      diffText.set(result.diff || '');
      toast($t('refresh') + ' · OK', 'success');
    } catch (err) {
      toast(err instanceof Error ? err.message : String(err), 'error');
    }
  }

  async function action(kind: 'apply' | 'reject' | 'undo') {
    busy = true;
    try {
      await api.diffAction(kind);
      toast(`${$t(kind)} · OK`, 'success');
      await reload();
    } catch (err) {
      toast(err instanceof Error ? err.message : String(err), 'error');
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex h-full min-h-0 flex-col">
  <div class="flex flex-none items-center gap-2 border-b border-border p-2">
    <Diff size={16} class="text-accent" />
    <div class="scroll-thin flex min-w-0 flex-1 gap-1 overflow-x-auto">
      {#each files as item, index}
        <button class="chip flex-none" class:chip-accent={index === active} on:click={() => (active = index)}>
          {item.path.split('/').pop()}
        </button>
      {/each}
      {#if files.length === 0}<span class="tiny muted">{$t('noChanges')}</span>{/if}
    </div>
    <button class="btn btn-icon btn-ghost" aria-label={$t('refresh')} on:click={reload}><Refresh size={16} /></button>
  </div>

  <div class="scroll-thin min-h-0 flex-1 overflow-auto p-2">
    {#if file}
      <div class="mb-2 flex items-center gap-2">
        <span class="mono min-w-0 flex-1 truncate text-[12px]">{file.path}</span>
        <span class="chip text-add-fg">+{file.added}</span>
        <span class="chip text-del-fg">−{file.removed}</span>
      </div>
      {#each file.hunks as hunk, index}
        <div class="mb-3 overflow-hidden rounded-sm border border-border">
          <button class="flex min-h-[40px] w-full items-center gap-2 bg-panel-2 px-2 text-left" on:click={() => (collapsed[index] = !collapsed[index])}>
            {#if collapsed[index]}<ChevronDown size={16} />{:else}<ChevronUp size={16} />{/if}
            <span class="mono tiny truncate">{hunk.header}</span>
          </button>
          {#if !collapsed[index]}
            <div class="mono overflow-x-auto text-[11.5px]">
              {#each hunk.lines as line}
                <div
                  class="px-2 whitespace-pre"
                  class:diff-line-add={line.kind === 'add'}
                  class:diff-line-del={line.kind === 'del'}
                >{line.kind === 'add' ? '+' : line.kind === 'del' ? '−' : ' '}{line.text}</div>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    {:else}
      <div class="flex h-full items-center justify-center text-[13px] muted">{$t('noChanges')}</div>
    {/if}
  </div>

  <div class="flex flex-none flex-wrap gap-2 border-t border-border p-2">
    <button class="btn btn-sm" disabled={busy} on:click={() => action('apply')}><Check size={16} /> {$t('apply')}</button>
    <button class="btn btn-sm" disabled={busy} on:click={() => action('reject')}><X size={16} /> {$t('reject')}</button>
    <button class="btn btn-sm" disabled={busy} on:click={() => action('undo')}><Undo size={16} /> {$t('undo')}</button>
  </div>
</div>
