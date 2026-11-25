<script lang="ts">
  import { Boxes, ChevronRight, GitBranch } from '../icons';
  import { subagents } from '../lib/store';
  import { t } from '../lib/i18n';

  let expanded: Record<string, boolean> = {};

  function children(node: any): any[] {
    return Array.isArray(node?.children) ? node.children : Array.isArray(node?.subagents) ? node.subagents : [];
  }
  function label(node: any): string {
    return node?.name || node?.agent || node?.title || 'subagent';
  }
</script>

<div class="flex h-full min-h-0 flex-col">
  <div class="flex min-h-[40px] flex-none items-center gap-2 border-b border-border px-3">
    <Boxes size={16} class="text-accent" />
    <span class="text-[12px] font-semibold">{$t('subagents')}</span>
    <span class="ml-auto text-[11px] muted">{$subagents.length}</span>
  </div>
  <div class="scroll-thin min-h-0 flex-1 overflow-y-auto p-3">
    {#if $subagents.length === 0}
      <div class="flex h-full items-center justify-center text-[13px] muted">{$t('noAgents')}</div>
    {:else}
      {#each $subagents as node, index (node.id || index)}
        <div class="mb-1">
          <button class="flex min-h-[40px] w-full items-center gap-2 rounded-sm px-2 text-left hover:bg-panel-2" on:click={() => (expanded[String(node.id || index)] = !expanded[String(node.id || index)])}>
            {#if children(node).length}<ChevronRight size={16} class={expanded[String(node.id || index)] ? 'rotate-90 text-accent' : 'text-muted'} />{:else}<GitBranch size={16} class="text-muted" />{/if}
            <span class="min-w-0 flex-1 truncate text-[13px]">{label(node)}</span>
            {#if node.status}<span class="chip tiny">{node.status}</span>{/if}
          </button>
          {#if expanded[String(node.id || index)]}
            <div class="ml-6 border-l border-border pl-2">
              {#each children(node) as child, childIndex}
                <div class="flex min-h-[36px] items-center gap-2 px-2 text-[12px] muted"><Boxes size={16} /> {label(child)}</div>
              {/each}
              {#if node.prompt}<pre class="mono m-2 max-h-32 overflow-auto whitespace-pre-wrap rounded-sm bg-bg p-2 text-[11px]">{node.prompt}</pre>{/if}
            </div>
          {/if}
        </div>
      {/each}
    {/if}
  </div>
</div>
