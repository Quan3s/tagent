<script lang="ts">
  import { Alert, Check, X, Edit, ChevronDown, ChevronUp } from '../icons';
  import { t } from '../lib/i18n';
  import type { ViewItem } from '../lib/view';

  export let event: ViewItem;
  export let onReply: (id: string, allow: boolean, always: boolean, edited: string) => void = () => {};
  export let active = false;

  let editing = false;
  let edited = '';

  $: id = String(event.id || event.permission_id || event.key);
  $: tool = event.tool || 'tool';
  $: target = event.target || '';
  $: resolved = !!event.resolved || !!event.answered;

  function reply(allow: boolean, always = false) {
    onReply(id, allow, always, editing ? edited : '');
    editing = false;
  }
</script>

<div class="card border-warn/60 p-3">
  <div class="flex items-start gap-2">
    <span class="mt-0.5 text-warn"><Alert size={20} /></span>
    <div class="min-w-0 flex-1">
      <div class="flex items-center justify-between gap-2">
        <strong class="text-[13px]">{$t('permissionTitle')}</strong>
        {#if resolved}<span class="chip chip-accent"><Check size={16} /> OK</span>
        {:else if active}<span class="chip chip-warn tiny">{$t('waiting')}</span>{/if}
      </div>
      <div class="tiny muted mono">{tool}</div>
      <pre class="mono mt-2 max-h-32 overflow-auto whitespace-pre-wrap rounded-sm border border-border bg-bg p-2 text-[12px]">{target}</pre>
      {#if editing}
        <textarea
          class="input mono mt-2"
          rows="3"
          aria-label={$t('editCommand')}
          bind:value={edited}
          placeholder={target}
        ></textarea>
      {/if}
      {#if !resolved}
        <div class="mt-3 flex flex-wrap gap-2">
          <button class="btn btn-primary btn-sm" on:click={() => reply(true)}><Check size={16} /> {$t('allow')}</button>
          <button class="btn btn-sm" on:click={() => reply(true, true)}><Check size={16} /> {$t('alwaysAllow')}</button>
          <button class="btn btn-danger btn-sm" on:click={() => reply(false)}><X size={16} /> {$t('deny')}</button>
          <button class="btn btn-ghost btn-sm" on:click={() => { editing = !editing; edited = edited || target; }}>
            {#if editing}<ChevronUp size={16} />{:else}<Edit size={16} />{/if} {$t('editCommand')}
          </button>
          {#if editing}
            <button class="btn btn-primary btn-sm" on:click={() => reply(true)}><Check size={16} /> {$t('send')}</button>
          {/if}
        </div>
      {/if}
    </div>
  </div>
</div>
