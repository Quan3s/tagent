<script lang="ts">
  import { X, Folder, Diff, Terminal, List, Boxes, Settings } from '../icons';
  import { panel, closePanel } from '../lib/store';
  import { t } from '../lib/i18n';
  import FilePanel from './FilePanel.svelte';
  import DiffPanel from './DiffPanel.svelte';
  import TerminalPanel from './TerminalPanel.svelte';
  import TodoPanel from './TodoPanel.svelte';
  import AgentPanel from './AgentPanel.svelte';
  import SettingsView from './Settings.svelte';

  export let settingsTab = 'providers';
  export let onProvidersChanged: () => void = () => {};
  export let onExtensionsChanged: () => void = () => {};

  $: title =
    $panel.type === 'files' ? $t('files')
    : $panel.type === 'diff' ? $t('diff')
    : $panel.type === 'terminal' ? $t('terminal')
    : $panel.type === 'todos' ? $t('todos')
    : $panel.type === 'agents' ? $t('subagents')
    : $panel.type === 'settings' ? $t('settings')
    : '';
</script>

{#if $panel.open && $panel.type}
  <div class="overlay mobile-only" role="presentation" on:click={closePanel}></div>
  <section class="flex h-full min-h-0 flex-col border-l border-border bg-panel" aria-label={title}>
    <div class="header-safe flex min-h-[52px] flex-none items-center gap-2 border-b border-border px-3">
      {#if $panel.type === 'files'}<Folder size={16} class="text-accent" />{/if}
      {#if $panel.type === 'diff'}<Diff size={16} class="text-accent" />{/if}
      {#if $panel.type === 'terminal'}<Terminal size={16} class="text-accent" />{/if}
      {#if $panel.type === 'todos'}<List size={16} class="text-accent" />{/if}
      {#if $panel.type === 'agents'}<Boxes size={16} class="text-accent" />{/if}
      {#if $panel.type === 'settings'}<Settings size={16} class="text-accent" />{/if}
      <strong class="min-w-0 flex-1 truncate text-[13px]">{title}</strong>
      <button class="btn btn-icon btn-ghost" aria-label={$t('close')} on:click={closePanel}><X size={20} /></button>
    </div>
    <div class="min-h-0 flex-1 overflow-hidden">
      {#if $panel.type === 'files'}<FilePanel />
      {:else if $panel.type === 'diff'}<DiffPanel />
      {:else if $panel.type === 'terminal'}<TerminalPanel />
      {:else if $panel.type === 'todos'}<TodoPanel />
      {:else if $panel.type === 'agents'}<AgentPanel />
      {:else if $panel.type === 'settings'}<SettingsView initialTab={settingsTab} {onProvidersChanged} {onExtensionsChanged} />
      {/if}
    </div>
  </section>
{/if}
