<script lang="ts">
  import {
    Menu,
    Diff,
    Terminal,
    List,
    GitBranch,
    Folder,
    Stop,
    Boxes,
    Cpu,
    More,
    Circle,
  } from '../icons';
  import { activeSession, busy, config, streamStatus } from '../lib/store';
  import { t } from '../lib/i18n';
  import type { PanelType } from '../lib/store';

  export let onToggleDrawer: () => void = () => {};
  export let onStop: () => void = () => {};
  export let onOpenPanel: (type: PanelType) => void = () => {};

  let menuOpen = false;

  $: repo = $activeSession?.repo || ($config?.workspace as string) || '';
  $: branch = $activeSession?.branch || '';
  $: status = $busy || $streamStatus === 'connecting' ? 'running' : $activeSession?.status || 'idle';
  $: statusLabel =
    status === 'running'
      ? $t('running')
      : status === 'waiting'
        ? $t('waiting')
        : status === 'error'
          ? $t('error')
          : status === 'done'
            ? $t('done')
            : $t('idle');
</script>

<header class="header-safe flex min-h-[60px] flex-none items-center gap-2 border-b border-border bg-panel px-3 md:px-4">
  <button class="btn btn-icon btn-ghost mobile-only" aria-label={$t('openMenu')} on:click={onToggleDrawer}>
    <Menu size={20} />
  </button>

  <div class="min-w-0 flex-1">
    <div class="flex items-center gap-2">
      <h1 class="truncate text-[14px] font-semibold leading-tight">{$activeSession?.title || 'ta'}</h1>
      <span class="flex flex-none items-center gap-1.5 rounded-pill border border-border px-2 py-0.5 text-[11px] muted">
        <span class={`dot ${status === 'running' ? 'dot-running' : status === 'error' ? 'dot-error' : 'dot-done'}`} aria-hidden="true"></span>
        {statusLabel}
      </span>
    </div>
    <div class="flex min-w-0 items-center gap-2 text-[11px] muted">
      {#if repo}<span class="truncate">{repo}</span>{/if}
      {#if repo && branch}<span aria-hidden="true">·</span>{/if}
      {#if branch}<span class="flex min-w-0 items-center gap-1 truncate text-accent"><GitBranch size={16} /> <span class="truncate">{branch}</span></span>{/if}
    </div>
  </div>

  <nav class="flex flex-none items-center gap-1" aria-label={$t('quickActions')}>
    <button class="btn btn-icon btn-ghost" aria-label={$t('files')} title={$t('files')} on:click={() => onOpenPanel('files')}>
      <Folder size={20} />
    </button>
    <button class="btn btn-icon btn-ghost" aria-label={$t('diff')} title={$t('diff')} on:click={() => onOpenPanel('diff')}>
      <Diff size={20} />
    </button>
    <button class="btn btn-icon btn-ghost" aria-label={$t('terminal')} title={$t('terminal')} on:click={() => onOpenPanel('terminal')}>
      <Terminal size={20} />
    </button>
    <div class="relative desktop-only">
      <button class="btn btn-icon btn-ghost" aria-label={$t('quickActions')} aria-expanded={menuOpen} on:click={() => (menuOpen = !menuOpen)}>
        <More size={20} />
      </button>
      {#if menuOpen}
        <div class="card absolute right-0 top-12 z-30 w-44 overflow-hidden p-1 shadow-lg">
          <button class="btn btn-ghost w-full justify-start" on:click={() => { onOpenPanel('todos'); menuOpen = false; }}><List size={16} /> {$t('todos')}</button>
          <button class="btn btn-ghost w-full justify-start" on:click={() => { onOpenPanel('agents'); menuOpen = false; }}><Boxes size={16} /> {$t('subagents')}</button>
          <button class="btn btn-ghost w-full justify-start" on:click={() => { onOpenPanel('settings'); menuOpen = false; }}><Cpu size={16} /> {$t('settings')}</button>
        </div>
      {/if}
    </div>
    <div class="mobile-only relative">
      <button class="btn btn-icon btn-ghost" aria-label={$t('quickActions')} aria-expanded={menuOpen} on:click={() => (menuOpen = !menuOpen)}>
        <More size={20} />
      </button>
      {#if menuOpen}
        <div class="card absolute right-2 top-12 z-30 w-44 overflow-hidden p-1 shadow-lg">
          <button class="btn btn-ghost w-full justify-start" on:click={() => { onOpenPanel('todos'); menuOpen = false; }}><List size={16} /> {$t('todos')}</button>
          <button class="btn btn-ghost w-full justify-start" on:click={() => { onOpenPanel('agents'); menuOpen = false; }}><Boxes size={16} /> {$t('subagents')}</button>
          <button class="btn btn-ghost w-full justify-start" on:click={() => { onOpenPanel('settings'); menuOpen = false; }}><Cpu size={16} /> {$t('settings')}</button>
        </div>
      {/if}
    </div>
    {#if $busy}
      <button class="btn btn-danger btn-icon" aria-label={$t('stop')} title={$t('stop')} on:click={onStop}>
        <Stop size={20} />
      </button>
    {:else}
      <span class="hidden items-center gap-1 text-[11px] muted sm:flex"><Circle size={16} /> {$t('ready')}</span>
    {/if}
  </nav>
</header>
