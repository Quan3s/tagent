<script lang="ts">
  import { Alert, Check, X, Edit, Send } from '../icons';
  import { permission } from '../lib/store';
  import { t } from '../lib/i18n';

  export let onReply: (id: string, allow: boolean, always: boolean, edited: string) => void = () => {};

  let editing = false;
  let edited = '';
  let lastId = '';

  $: if ($permission && $permission.id !== lastId) {
    lastId = $permission.id;
    editing = false;
    edited = $permission.target || '';
  }

  function reply(allow: boolean, always = false) {
    if (!$permission) return;
    onReply($permission.id, allow, always, editing ? edited : '');
    editing = false;
  }
</script>

{#if $permission && !$permission.resolved}
  <div class="overlay" role="presentation"></div>
  <div class="sheet" role="dialog" aria-modal="true" aria-label={$t('permissionTitle')}>
    <div class="sheet-grip" aria-hidden="true"></div>
    <div class="flex items-center gap-2 border-b border-border px-4 py-3">
      <span class="text-warn"><Alert size={20} /></span>
      <div class="min-w-0 flex-1">
        <div class="font-semibold">{$t('permissionTitle')}</div>
        <div class="tiny muted">{$t('permissionHint')}</div>
      </div>
    </div>
    <div class="scroll-thin min-h-0 flex-1 overflow-y-auto p-4">
      <div class="mb-2 flex items-center gap-2">
        <span class="chip chip-warn">{$permission.tool}</span>
        {#if $permission.decision}<span class="chip">{$permission.decision}</span>{/if}
      </div>
      <pre class="mono max-h-48 overflow-auto whitespace-pre-wrap rounded-sm border border-border bg-bg p-3 text-[12px]">{$permission.target}</pre>
      {#if editing}
        <label class="label mt-3" for="perm-edit">{$t('editCommand')}</label>
        <textarea id="perm-edit" class="input mono" rows="4" bind:value={edited}></textarea>
      {/if}
    </div>
    <div class="flex flex-wrap gap-2 border-t border-border p-3">
      <button class="btn btn-primary flex-1" on:click={() => reply(true)}><Check size={16} /> {$t('allow')}</button>
      <button class="btn flex-1" on:click={() => reply(true, true)}><Check size={16} /> {$t('alwaysAllow')}</button>
      <button class="btn btn-danger flex-1" on:click={() => reply(false)}><X size={16} /> {$t('deny')}</button>
      <button class="btn btn-ghost w-full" on:click={() => (editing = !editing)}>
        {#if editing}<X size={16} />{:else}<Edit size={16} />{/if} {$t('editCommand')}
      </button>
      {#if editing}
        <button class="btn btn-primary w-full" on:click={() => reply(true)}><Send size={16} /> {$t('send')}</button>
      {/if}
    </div>
  </div>
{/if}
