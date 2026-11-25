<script lang="ts">
  import { afterUpdate, tick } from 'svelte';
  import { Sparkles, ArrowUp } from '../icons';
  import MessageRow from './MessageRow.svelte';
  import { buildViewItems } from '../lib/view';
  import { computeWindow } from '../lib/virtual';
  import { events, loading, busy } from '../lib/store';
  import { t } from '../lib/i18n';

  export let onSuggestion: (text: string) => void = () => {};
  export let onPermissionReply: (id: string, allow: boolean, always: boolean, edited: string) => void = () => {};
  export let onOpenDiff: () => void = () => {};
  export let activePermissionId = '';

  let container: HTMLDivElement;
  let scrollTop = 0;
  let viewportH = 600;
  let heights: Record<string, number> = {};
  let autoScroll = true;
  let lastFingerprint = '';

  $: items = buildViewItems($events);
  $: heightList = items.map((item) => heights[item.key] || 150);
  $: win =
    items.length > 200
      ? computeWindow(heightList, scrollTop, viewportH, 150, 6)
      : { start: 0, end: items.length, padTop: 0, padBottom: 0, total: 0 };
  $: visible = items.slice(win.start, win.end);

  afterUpdate(() => {
    if (!container) return;
    let changed = false;
    const h = container.clientHeight;
    if (Math.abs(h - viewportH) > 2) {
      viewportH = h;
      changed = true;
    }
    container.querySelectorAll<HTMLElement>('[data-event-key]').forEach((node) => {
      const key = node.dataset.eventKey || '';
      const measured = node.getBoundingClientRect().height;
      if (measured > 0 && Math.abs((heights[key] || 150) - measured) > 2) {
        heights = { ...heights, [key]: measured };
        changed = true;
      }
    });
    if (changed) scrollTop = container.scrollTop;
  });

  function onScroll() {
    if (!container) return;
    scrollTop = container.scrollTop;
    autoScroll = scrollTop + container.clientHeight >= container.scrollHeight - 160;
  }

  function scrollBottom() {
    if (container) container.scrollTop = container.scrollHeight;
  }

  $: watch($events);
  function watch(list: typeof $events) {
    const last = list[list.length - 1];
    const fingerprint = `${list.length}:${last?.key || ''}:${String(last?.text || '').length}`;
    if (fingerprint === lastFingerprint) return;
    lastFingerprint = fingerprint;
    if (autoScroll) void tick().then(scrollBottom);
  }

  const suggestions = ['suggestion1', 'suggestion2', 'suggestion3'];
</script>

<div
  class="scroll-thin min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 py-4 md:px-6"
  bind:this={container}
  on:scroll={onScroll}
  role="log"
  aria-label={$t('sessions')}
>
  <div class="mx-auto w-full max-w-[760px]">
    {#if $loading}
      <div class="space-y-4 py-6" aria-busy="true">
        {#each [0, 1, 2, 3] as row}
          <div class="flex gap-3">
            <div class="skeleton h-8 w-8 flex-none rounded-sm"></div>
            <div class="flex-1 space-y-2">
              <div class="skeleton h-3 w-2/3"></div>
              <div class="skeleton h-3 w-11/12"></div>
              <div class="skeleton h-3 w-1/2"></div>
            </div>
          </div>
        {/each}
      </div>
    {:else if items.length === 0}
      <div class="flex min-h-[52vh] flex-col items-center justify-center text-center">
        <div class="mb-4 flex h-14 w-14 items-center justify-center rounded-lg border border-accent/30 bg-accent/10 text-accent">
          <Sparkles size={20} />
        </div>
        <h2 class="text-lg font-semibold">{$t('emptyTitle')}</h2>
        <p class="mt-2 max-w-sm text-[13px] muted">{$t('emptyHint')}</p>
        <div class="mt-6 flex w-full max-w-md flex-col gap-2">
          {#each suggestions as key}
            <button class="card flex min-h-[44px] items-center gap-2 px-3 text-left text-[13px] hover:border-accent" on:click={() => onSuggestion($t(key))}>
              <ArrowUp size={16} class="text-accent" /> {$t(key)}
            </button>
          {/each}
        </div>
      </div>
    {:else}
      {#if win.padTop > 0}<div style={`height:${win.padTop}px`} aria-hidden="true"></div>{/if}
      {#each visible as item (item.key)}
        <MessageRow
          {item}
          {onPermissionReply}
          {onOpenDiff}
          activePermissionId={activePermissionId}
        />
      {/each}
      {#if win.padBottom > 0}<div style={`height:${win.padBottom}px`} aria-hidden="true"></div>{/if}
      {#if $busy}
        <div class="mb-4 flex items-center gap-2 text-[13px] muted">
          <Sparkles size={16} class="spin text-accent" /> <span class="animate-pulse">…</span>
        </div>
      {/if}
    {/if}
  </div>
</div>
