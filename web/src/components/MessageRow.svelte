<script lang="ts">
  import { Sparkles, ChevronDown, ChevronUp, File, Alert, CheckCircle, Info, Boxes, Activity, Paperclip } from '../icons';
  import Markdown from './Markdown.svelte';
  import ToolCard from './ToolCard.svelte';
  import PermissionCard from './PermissionCard.svelte';
  import DiffCard from './DiffCard.svelte';
  import { t } from '../lib/i18n';
  import type { ViewItem } from '../lib/view';

  export let item: ViewItem;
  export let onPermissionReply: (id: string, allow: boolean, always: boolean, edited: string) => void = () => {};
  export let activePermissionId = '';
  export let onOpenDiff: () => void = () => {};

  let thinkingOpen = false;
  $: isUser = item.role === 'user';
  $: attachments = Array.isArray(item.attachments) ? item.attachments : [];
  $: todos = Array.isArray(item.items) ? item.items : [];
</script>

<div data-event-key={item.key}>
  {#if item.type === 'message'}
    {#if isUser}
      <div class="mb-4 flex flex-col items-end gap-2">
        <div class="user-bubble">{item.text}</div>
        {#if attachments.length}
          <div class="flex max-w-[88%] flex-wrap justify-end gap-1.5">
            {#each attachments as attachment}
              <span class="chip chip-accent"><Paperclip size={16} /> {typeof attachment === 'string' ? attachment : attachment.path || attachment.name}</span>
            {/each}
          </div>
        {/if}
      </div>
    {:else}
      <div class="agent-block mb-4 flex gap-2.5">
        <span class="mt-1 hidden h-8 w-8 flex-none items-center justify-center rounded-sm border border-accent/30 bg-accent/10 text-accent sm:flex">
          <Sparkles size={16} />
        </span>
        <div class="min-w-0 flex-1">
          {#if item.text}<Markdown text={item.text} />{/if}
          {#if item.streaming && !item.text}<span class="muted inline-flex items-center gap-2"><Activity size={16} class="spin" /> …</span>{/if}
        </div>
      </div>
    {/if}
  {:else if item.type === 'thinking'}
    <div class="mb-3">
      <button class="btn btn-ghost btn-sm w-full justify-start" aria-expanded={thinkingOpen} on:click={() => (thinkingOpen = !thinkingOpen)}>
        {#if thinkingOpen}<ChevronUp size={16} />{:else}<ChevronDown size={16} />{/if}
        <Sparkles size={16} class="text-muted" />
        <span class="tiny muted">{$t('thinking')}</span>
        {#if !thinkingOpen}<span class="tiny muted truncate">{String(item.text || '').slice(0, 80)}</span>{/if}
      </button>
      {#if thinkingOpen}
        <div class="ml-4 border-l-2 border-border pl-3 pt-1 text-[13px] muted italic whitespace-pre-wrap">{item.text}</div>
      {/if}
    </div>
  {:else if item.type === 'tool'}
    <div class="mb-3"><ToolCard event={item} /></div>
  {:else if item.type === 'permission.request' || item.type === 'permission'}
    <div class="mb-3"><PermissionCard event={item} active={activePermissionId === String(item.id || item.key)} onReply={onPermissionReply} /></div>
  {:else if item.type === 'diff'}
    <div class="mb-3"><DiffCard event={item} onOpen={onOpenDiff} /></div>
  {:else if item.type === 'error'}
    <div class="mb-3 flex gap-2 rounded-sm border border-danger/50 bg-danger/10 p-3 text-[13px] text-danger">
      <Alert size={16} class="flex-none" /> <span class="overflow-wrap-anywhere">{item.message || item.text || $t('error')}</span>
    </div>
  {:else if item.type === 'usage'}
    <div class="mb-2 flex flex-wrap items-center gap-2 text-[11px] muted">
      <Activity size={16} />
      <span>{item.input_tokens ?? 0} in · {item.output_tokens ?? 0} out {$t('tokens')}</span>
      {#if item.cost_usd !== undefined}<span class="text-accent">${Number(item.cost_usd).toFixed(4)}</span>{/if}
    </div>
  {:else if item.type === 'done'}
    <div class="my-4 flex items-center gap-2 text-[11px] muted">
      <CheckCircle size={16} class="text-accent" />
      <span>{$t('done')}</span>
      {#if item.turns !== undefined}<span>· {item.turns} {$t('turn')}</span>{/if}
      {#if item.reason}<span>· {item.reason}</span>{/if}
    </div>
  {:else if item.type === 'todo.update'}
    <div class="card mb-3 p-3">
      <div class="mb-2 flex items-center gap-2 text-[12px] font-semibold"><Boxes size={16} /> {$t('todos')}</div>
      <ul class="space-y-1 text-[12px]">
        {#each todos as todo}
          <li class="flex items-start gap-2">
            <span class="mt-1 {todo.status === 'completed' || todo.done ? 'text-accent' : 'text-muted'}">
              {#if todo.status === 'completed' || todo.done}<CheckCircle size={16} />{:else}<Info size={16} />{/if}
            </span>
            <span>{todo.content || todo.text || String(todo)}</span>
          </li>
        {/each}
      </ul>
    </div>
  {:else if item.type.startsWith('subagent')}
    <div class="card mb-3 flex items-center gap-2 p-3 text-[12px]">
      <Boxes size={16} class="text-accent" />
      <span class="truncate">{item.agent || item.name || 'subagent'}</span>
      {#if item.status}<span class="chip ml-auto">{item.status}</span>{/if}
    </div>
  {:else if item.type === 'file'}
    <div class="mb-2 inline-flex items-center gap-2 chip"><File size={16} /> {item.path || item.name}</div>
  {/if}
</div>
