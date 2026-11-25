<script lang="ts">
  import { tick } from 'svelte';
  import { Search, Terminal, File, Sparkles, Plug, CornerDownLeft } from '../icons';
  import { extensions, palette, tree } from '../lib/store';
  import { t } from '../lib/i18n';

  export let onSelect: (token: string) => void = () => {};

  const builtins = [
    'help', 'model', 'clear', 'compact', 'cost', 'context', 'resume', 'undo', 'redo', 'diff', 'plan',
    'init', 'permissions', 'plugin', 'skills', 'agents', 'mcp', 'hooks', 'config', 'theme', 'doctor',
    'export', 'quit',
  ];

  let input: HTMLInputElement;
  let selected = 0;

  interface Item { value: string; label: string; hint: string; kind: string }
  $: items = buildItems($palette.kind, $palette.query, $tree, $extensions);
  $: if ($palette.open) void tick().then(() => input?.focus());
  $: if ($palette.query) selected = 0;

  function buildItems(kind: string, query: string, treeItems: typeof $tree, ext: typeof $extensions): Item[] {
    const q = query.trim().toLowerCase();
    let all: Item[];
    if (kind === 'file') {
      all = treeItems.map((node) => {
        const isDir = node.type === 'dir' || node.type === 'directory';
        return { value: `@${node.path}${isDir ? '/' : ''}`, label: node.path, hint: isDir ? 'dir' : 'file', kind: 'file' };
      });
    } else {
      const commands: Item[] = builtins.map((cmd) => ({ value: `/${cmd} `, label: `/${cmd}`, hint: 'built-in', kind: 'command' }));
      for (const p of ext.plugins || []) commands.push({ value: `/plugin:${p.name} `, label: `/plugin:${p.name}`, hint: p.description || 'plugin', kind: 'plugin' });
      for (const s of ext.skills || []) commands.push({ value: `/${s.name} `, label: `/${s.name}`, hint: s.description || 'skill', kind: 'skill' });
      for (const a of ext.agents || []) commands.push({ value: `@${a.name} `, label: `@${a.name}`, hint: a.description || 'agent', kind: 'agent' });
      all = commands;
    }
    return q ? all.filter((i) => `${i.label} ${i.hint}`.toLowerCase().includes(q)).slice(0, 80) : all.slice(0, 80);
  }

  function close() {
    palette.update((p) => ({ ...p, open: false, query: '' }));
  }
  function choose(item: Item) {
    onSelect(item.value);
    close();
  }
  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      selected = Math.min(items.length - 1, selected + 1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      selected = Math.max(0, selected - 1);
    } else if (event.key === 'Enter' && items[selected]) {
      event.preventDefault();
      choose(items[selected]);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      close();
    }
  }
</script>

{#if $palette.open}
  <div class="overlay" role="presentation" on:click={close}></div>
  <div class="sheet" role="dialog" aria-modal="true" aria-label={$palette.kind === 'file' ? $t('files') : $t('commands')}>
    <div class="sheet-grip" aria-hidden="true"></div>
    <div class="flex items-center gap-2 border-b border-border px-4 py-3">
      <Search size={20} class="text-accent" />
      <input
        bind:this={input}
        class="w-full border-0 bg-transparent outline-none placeholder:text-muted"
        aria-label={$palette.kind === 'file' ? $t('searchFiles') : $t('searchCommands')}
        placeholder={$palette.kind === 'file' ? $t('searchFiles') : $t('searchCommands')}
        value={$palette.query}
        on:input={(e) => palette.update((p) => ({ ...p, query: e.currentTarget.value }))}
        on:keydown={onKeydown}
      />
      <span class="tiny muted">Esc</span>
    </div>
    <div class="scroll-thin min-h-0 flex-1 overflow-y-auto p-2" role="listbox" aria-label={$palette.kind === 'file' ? $t('files') : $t('commands')}>
      {#if items.length === 0}
        <div class="p-6 text-center text-[13px] muted">{$t('noFiles')}</div>
      {:else}
        {#each items as item, index}
          <button
            class="flex min-h-[44px] w-full items-center gap-3 rounded-sm px-3 text-left hover:bg-panel-2"
            class:bg-panel-2={index === selected}
            role="option"
            aria-selected={index === selected}
            on:click={() => choose(item)}
            on:mouseenter={() => (selected = index)}
          >
            <span class="flex-none text-accent">
              {#if item.kind === 'file'}<File size={16} />{:else if item.kind === 'plugin'}<Plug size={16} />{:else if item.kind === 'skill'}<Sparkles size={16} />{:else}<Terminal size={16} />{/if}
            </span>
            <span class="min-w-0 flex-1 truncate mono text-[13px]">{item.label}</span>
            <span class="tiny muted max-w-[45%] truncate">{item.hint}</span>
            {#if index === selected}<CornerDownLeft size={16} class="text-muted" />{/if}
          </button>
        {/each}
      {/if}
    </div>
  </div>
{/if}
