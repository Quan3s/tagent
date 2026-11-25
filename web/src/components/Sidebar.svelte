<script lang="ts">
  import { Sparkles, Plus, Settings, Plug, Boxes, Server, User, X, Cpu } from '../icons';
  import {
    activeId,
    extensionCounts,
    lang,
    model,
    sessions,
    usage,
    drawer,
    currentModel,
  } from '../lib/store';
  import { t } from '../lib/i18n';

  export let onNewTask: () => void = () => {};
  export let onSelect: (id: string) => void = () => {};
  export let onOpenSettings: (tab?: string) => void = () => {};

  function statusClass(status?: string): string {
    if (status === 'running') return 'dot dot-running';
    if (status === 'waiting') return 'dot dot-waiting';
    if (status === 'error') return 'dot dot-error';
    return 'dot dot-done';
  }
  function when(value?: string | number): string {
    if (!value) return '';
    const d = new Date(value);
    return Number.isNaN(d.getTime()) ? '' : d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  }
  function shortModel(value: string): string {
    return value.includes('/') ? value.slice(value.indexOf('/') + 1) : value || '—';
  }
</script>

<aside class="flex h-full flex-col bg-panel">
  <div class="header-safe flex items-center gap-3 border-b border-border px-4 py-3">
    <div
      class="flex h-10 w-10 flex-none items-center justify-center rounded-md border border-accent/40 bg-accent/10 text-accent"
    >
      <Sparkles size={20} />
    </div>
    <div class="min-w-0 flex-1">
      <div class="text-[15px] font-bold leading-tight tracking-tight">ta<span class="text-accent">.</span></div>
      <div class="tiny muted leading-tight">coding agent</div>
    </div>
    <button class="btn btn-icon btn-ghost mobile-only" aria-label={$t('close')} on:click={() => drawer.set(false)}>
      <X size={20} />
    </button>
  </div>

  <div class="px-3 pb-2 pt-3">
    <button class="btn btn-primary w-full justify-start" on:click={() => { onNewTask(); drawer.set(false); }}>
      <Plus size={20} /> {$t('newTask')}
    </button>
  </div>

  <div class="scroll-thin min-h-0 flex-1 overflow-y-auto px-3 pb-3">
    <div class="mb-2 px-2 pt-2 text-[11px] font-semibold uppercase tracking-wider muted">{$t('sessions')}</div>
    {#if $sessions.length === 0}
      <div class="card m-1 p-3 text-center">
        <div class="mb-1 font-medium">{$t('noSessions')}</div>
        <div class="tiny muted">{$t('noSessionsHint')}</div>
      </div>
    {:else}
      <nav aria-label={$t('sessions')} class="flex flex-col gap-1">
        {#each $sessions as session (session.id)}
          <button
            class={`flex min-h-[44px] items-center gap-2 rounded-sm border px-2 py-2 text-left transition-colors ${
              $activeId === session.id ? 'border-accent bg-panel-2' : 'border-transparent hover:bg-panel-2'
            }`}
            on:click={() => { onSelect(session.id); drawer.set(false); }}
          >
            <span class={statusClass(session.status)} aria-hidden="true"></span>
            <span class="min-w-0 flex-1">
              <span class="block truncate text-[13px] font-medium">{session.title || $t('newTask')}</span>
              {#if when(session.updated_at)}
                <span class="tiny muted block">{when(session.updated_at)}</span>
              {/if}
            </span>
          </button>
        {/each}
      </nav>
    {/if}

    <div class="mb-2 mt-5 px-2 text-[11px] font-semibold uppercase tracking-wider muted">{$t('extensions') ?? 'Extensions'}</div>
    <div class="grid grid-cols-2 gap-2">
      <button class="card flex min-h-[44px] items-center gap-2 px-3 text-left text-[13px] hover:border-accent" on:click={() => onOpenSettings('plugins')}>
        <Plug size={16} class="text-accent" /> <span class="flex-1">{$t('plugins')}</span>
        <span class="tiny muted">{$extensionCounts.plugins}</span>
      </button>
      <button class="card flex min-h-[44px] items-center gap-2 px-3 text-left text-[13px] hover:border-accent" on:click={() => onOpenSettings('skills')}>
        <Sparkles size={16} class="text-accent" /> <span class="flex-1">{$t('skills')}</span>
        <span class="tiny muted">{$extensionCounts.skills}</span>
      </button>
      <button class="card flex min-h-[44px] items-center gap-2 px-3 text-left text-[13px] hover:border-accent" on:click={() => onOpenSettings('mcp')}>
        <Server size={16} class="text-accent" /> <span class="flex-1">{$t('mcp')}</span>
        <span class="tiny muted">{$extensionCounts.mcp}</span>
      </button>
      <button class="card flex min-h-[44px] items-center gap-2 px-3 text-left text-[13px] hover:border-accent" on:click={() => { onOpenSettings('providers'); }}>
        <User size={16} class="text-accent" /> <span class="flex-1">{$t('agents')}</span>
        <span class="tiny muted">{$extensionCounts.agents}</span>
      </button>
    </div>
  </div>

  <div class="border-t border-border px-3 py-2">
    <button class="flex w-full items-center gap-2 rounded-sm px-2 py-2 text-left hover:bg-panel-2" on:click={() => onOpenSettings('providers')}>
      <Cpu size={16} class="text-muted" />
      <span class="min-w-0 flex-1">
        <span class="tiny muted block">{$t('model')}</span>
        <span class="block truncate text-[12px]">{shortModel($model || currentModel())}</span>
      </span>
    </button>
    <div class="flex items-center justify-between px-2 pb-1">
      <span class="tiny muted">{$t('todayCost')}</span>
      <span class="mono text-[12px] text-accent">${($usage.cost_usd || 0).toFixed(2)}</span>
    </div>
    <button class="btn btn-ghost w-full justify-start" on:click={() => onOpenSettings('appearance')}>
      <Settings size={16} /> {$t('settings')}
    </button>
    <div class="pb-[env(safe-area-inset-bottom,0px)]"></div>
  </div>
</aside>
