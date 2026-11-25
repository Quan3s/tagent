<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { Terminal as TerminalIcon, Info } from '../icons';
  import { createTerminal, type TerminalHandle } from '../lib/lazy/terminal';
  import { t } from '../lib/i18n';

  let host: HTMLDivElement;
  let handle: TerminalHandle | null = null;
  let supported = true;

  async function mount() {
    if (!host) return;
    handle?.dispose();
    host.innerHTML = '';
    handle = await createTerminal(host);
    supported = handle.supported;
    handle.write(`ta · ${$t('terminalHint')}\r\n$ `);
  }

  void tick().then(mount);
  onDestroy(() => handle?.dispose());
</script>

<div class="flex h-full min-h-0 flex-col">
  <div class="flex min-h-[40px] flex-none items-center gap-2 border-b border-border px-3">
    <TerminalIcon size={16} class="text-accent" />
    <span class="text-[12px] font-semibold">{$t('terminal')}</span>
    <span class="ml-auto text-[11px] muted">xterm.js</span>
  </div>
  <div class="min-h-0 flex-1 overflow-hidden p-2" bind:this={host}></div>
  {#if !supported}
    <div class="flex items-center gap-2 border-t border-border px-3 py-2 text-[11px] muted">
      <Info size={16} /> {$t('terminalUnavailable')}
    </div>
  {/if}
</div>
