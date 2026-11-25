<script lang="ts">
  import { tick } from 'svelte';
  import { Paperclip, Send, Stop, ChevronDown, File, X, Zap, Keyboard, Sparkles } from '../icons';
  import { api } from '../lib/api';
  import {
    attachments,
    busy,
    config,
    currentModel,
    model,
    palette,
    permissionMode,
    providers,
    shareDraft,
    toast,
  } from '../lib/store';
  import { t } from '../lib/i18n';
  import type { UploadResult } from '../lib/types';

  export let onSend: (payload: {
    text: string;
    model: string;
    permissionMode: string;
    attachments: string[];
  }) => void = () => {};
  export let onInterrupt: () => void = () => {};
  export let focusSignal = 0;

  let value = '';
  let area: HTMLTextAreaElement;
  let fileInput: HTMLInputElement;
  let uploading = false;
  let prefixStart: number | null = null;
  let prefixEnd = 0;

  $: options = buildModelOptions($providers, $config);

  function buildModelOptions(list: typeof $providers, cfg: Record<string, any>): string[] {
    const set = new Set<string>();
    for (const p of list) {
      for (const m of p.models || []) set.add(m.includes('/') ? m : `${p.name}/${m}`);
      if (!p.models?.length) set.add(`${p.name}/default`);
    }
    const roles = cfg?.roles || {};
    for (const role of Object.values(roles)) if (typeof role === 'string' && role.includes('/')) set.add(role);
    const chosen = $model || currentModel();
    if (chosen) set.add(chosen);
    return [...set].sort();
  }

  function resize() {
    if (!area) return;
    area.style.height = 'auto';
    area.style.height = `${Math.min(180, Math.max(44, area.scrollHeight))}px`;
  }

  export function focus(): Promise<void> {
    return tick().then(() => area?.focus());
  }

  /** Called by the command/file palette with the chosen token. */
  export function insertToken(token: string): void {
    const text = value;
    if (prefixStart !== null) {
      value = text.slice(0, prefixStart) + token + text.slice(prefixEnd);
      const caret = prefixStart + token.length;
      prefixStart = null;
      void tick().then(() => {
        area?.focus();
        area?.setSelectionRange(caret, caret);
        resize();
      });
    } else {
      const start = area?.selectionStart ?? value.length;
      const end = area?.selectionEnd ?? value.length;
      value = text.slice(0, start) + token + text.slice(end);
      void tick().then(() => {
        area?.focus();
        area?.setSelectionRange(start + token.length, start + token.length);
        resize();
      });
    }
  }

  function openPalette(kind: 'command' | 'file', query = '', typedPrefix = false) {
    const caret = area?.selectionStart ?? value.length;
    prefixStart = typedPrefix ? Math.max(0, caret - 1) : (area?.selectionStart ?? value.length);
    prefixEnd = area?.selectionEnd ?? caret;
    palette.set({ open: true, kind, query });
  }

  function handleInput(event: Event) {
    resize();
    const input = event as InputEvent;
    const data = input.data || '';
    if (data === '/' && (value === '/' || /\s\/$/.test(value))) openPalette('command', '', true);
    else if (data === '@') openPalette('file', '', true);
    else if (data === '#' || data === '!') {
      toast(data === '#' ? $t('memoryHint') : $t('shellHint'), 'info');
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      if ($palette.open) {
        event.stopPropagation();
        palette.update((p) => ({ ...p, open: false }));
        prefixStart = null;
      } else if ($busy) {
        event.stopPropagation();
        onInterrupt();
      }
      return;
    }
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      openPalette('command');
      return;
    }
    if (event.key === 'Enter' && (event.shiftKey === false || event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      submit();
    }
  }

  function submit() {
    const text = value.trim();
    if (!text && !$attachments.length) return;
    onSend({
      text,
      model: $model || currentModel(),
      permissionMode: $permissionMode,
      attachments: $attachments.map((a) => a.path),
    });
    value = '';
    attachments.set([]);
    resize();
  }

  async function upload(files: FileList | File[] | null) {
    if (!files?.length) return;
    uploading = true;
    for (const file of Array.from(files)) {
      try {
        const result: UploadResult = await api.upload(file);
        attachments.update((list) => [...list, result]);
        insertToken(`@${result.path} `);
        toast(`${$t('uploaded')}: ${result.name}`, 'success');
      } catch (err) {
        toast(`${$t('uploadFailed')}: ${err instanceof Error ? err.message : String(err)}`, 'error');
      }
    }
    uploading = false;
  }

  function handlePaste(event: ClipboardEvent) {
    const files = Array.from(event.clipboardData?.files || []).filter((f) => f.type.startsWith('image/'));
    if (files.length) {
      event.preventDefault();
      void upload(files);
    }
  }

  function handleDrop(event: DragEvent) {
    const files = event.dataTransfer?.files;
    if (files?.length) {
      event.preventDefault();
      void upload(files);
    }
  }

  function removeAttachment(fileId: string) {
    attachments.update((list) => list.filter((a) => a.file_id !== fileId));
  }

  $: if (focusSignal) void focus();

  $: consumeShare($shareDraft);
  function consumeShare(draft: { text: string; files: File[] }) {
    if (!draft.text && !draft.files.length) return;
    value = draft.text || value;
    shareDraft.set({ text: '', files: [] });
    toast($t('shareReceived'), 'success');
    void tick().then(() => {
      focus();
      resize();
    });
    if (draft.files.length) void upload(draft.files);
  }
</script>

<div class="composer-safe flex-none border-t border-border bg-panel px-3 pt-2 md:px-6">
  <div class="mx-auto w-full max-w-[760px]">
    <div class="mb-2 flex flex-wrap items-center gap-2">
      <label class="sr-only" for="composer-mode">{$t('permissionMode')}</label>
      <select id="composer-mode" class="chip cursor-pointer appearance-none pr-3" bind:value={$permissionMode} aria-label={$t('permissionMode')}>
        <option value="ask">{$t('ask')}</option>
        <option value="auto-edit">{$t('autoEdit')}</option>
        <option value="plan">{$t('plan')}</option>
        <option value="yolo">{$t('yolo')}</option>
      </select>
      <label class="sr-only" for="composer-model">{$t('model')}</label>
      <select id="composer-model" class="chip chip-accent max-w-[55vw] cursor-pointer appearance-none truncate pr-3" aria-label={$t('model')} on:change={(e) => model.set(e.currentTarget.value)}>
        {#each options as option}<option value={option} selected={option === ($model || currentModel())}>{option}</option>{/each}
      </select>
      <button class="chip cursor-pointer" on:click={() => openPalette('command')} aria-label={$t('commands')}>
        <Keyboard size={16} /> /
      </button>
      <span class="ml-auto tiny muted hidden sm:inline">Ctrl+K · Ctrl+Enter</span>
    </div>

    {#if $attachments.length}
      <div class="mb-2 flex flex-wrap gap-2">
        {#each $attachments as attachment (attachment.file_id)}
          <span class="chip chip-accent"><File size={16} /> <span class="max-w-[140px] truncate">{attachment.name || attachment.path}</span>
            <button class="btn btn-ghost btn-sm" style="min-height:24px;width:24px" aria-label={`${$t('remove')} ${attachment.name}`} on:click={() => removeAttachment(attachment.file_id)}><X size={16} /></button>
          </span>
        {/each}
      </div>
    {/if}

    <div class="card flex items-end gap-2 p-2">
      <button class="btn btn-icon btn-ghost" aria-label={$t('attach')} title={$t('attach')} disabled={uploading} on:click={() => fileInput.click()}>
        {#if uploading}<Sparkles size={20} class="spin" />{:else}<Paperclip size={20} />{/if}
      </button>
      <input bind:this={fileInput} type="file" multiple class="hidden" on:change={(e) => void upload(e.currentTarget.files)} />
      <textarea
        bind:this={area}
        bind:value
        rows="1"
        class="max-h-[180px] min-h-[44px] w-full flex-1 resize-none border-0 bg-transparent px-1 py-2 text-[14px] outline-none placeholder:text-muted"
        placeholder={$t('promptPlaceholder')}
        aria-label={$t('promptPlaceholder')}
        on:input={handleInput}
        on:keydown={handleKeydown}
        on:paste={handlePaste}
        on:drop={handleDrop}
        on:dragover|preventDefault
      ></textarea>
      {#if $busy}
        <button class="btn btn-danger btn-icon" aria-label={$t('stop')} on:click={onInterrupt}><Stop size={20} /></button>
      {:else}
        <button class="btn btn-primary btn-icon" aria-label={$t('send')} disabled={!value.trim() && !$attachments.length} on:click={submit}><Send size={20} /></button>
      {/if}
    </div>
  </div>
</div>
