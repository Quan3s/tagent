<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { Folder, FolderOpen, File, Search, Copy, Refresh } from '../icons';
  import { api } from '../lib/api';
  import { tree, panel, toast } from '../lib/store';
  import { t } from '../lib/i18n';
  import { copyText } from '../lib/clipboard';
  import { createCodeEditor, type EditorHandle } from '../lib/lazy/codeEditor';
  import type { TreeNode } from '../lib/types';

  let query = '';
  let selected = '';
  let content = '';
  let loadingFile = false;
  let host: HTMLDivElement;
  let editor: EditorHandle | null = null;
  let supported = true;

  $: flat = flatten($tree).filter((node) => node.path.toLowerCase().includes(query.toLowerCase())).slice(0, 400);
  $: if ($panel.open && $panel.type === 'files' && $panel.filePath && $panel.filePath !== selected) {
    void openFile($panel.filePath);
  }

  function flatten(nodes: TreeNode[], depth = 0): (TreeNode & { depth: number })[] {
    const out: (TreeNode & { depth: number })[] = [];
    for (const node of [...nodes].sort((a, b) => a.path.localeCompare(b.path))) {
      out.push({ ...node, depth });
      if (Array.isArray((node as any).children)) out.push(...flatten((node as any).children, depth + 1));
    }
    return out;
  }

  function isDir(node: TreeNode): boolean {
    return node.type === 'dir' || node.type === 'directory';
  }

  async function openFile(path: string) {
    selected = path;
    loadingFile = true;
    content = '';
    try {
      content = await api.file(path);
      loadingFile = false;
      await tick();
      await mountEditor();
    } catch (err) {
      toast(err instanceof Error ? err.message : String(err), 'error');
      loadingFile = false;
    }
  }

  async function mountEditor() {
    editor?.destroy();
    editor = null;
    if (!host) return;
    host.innerHTML = '';
    editor = await createCodeEditor(host, content);
    supported = editor.supported;
  }

  async function copyFile() {
    if (await copyText(content)) toast($t('copied'), 'success');
  }

  function reload() {
    void mountEditor();
  }

  onDestroy(() => editor?.destroy());
</script>

<div class="flex h-full min-h-0 flex-col">
  <div class="flex flex-none items-center gap-2 border-b border-border p-2">
    <div class="relative flex-1">
      <Search size={16} class="absolute left-2 top-3 text-muted" />
      <input class="input pl-8" placeholder={$t('searchFiles')} aria-label={$t('searchFiles')} bind:value={query} />
    </div>
    <button class="btn btn-icon btn-ghost" aria-label={$t('refresh')} on:click={reload}><Refresh size={16} /></button>
  </div>
  <div class="scroll-thin min-h-0 flex-1 overflow-y-auto p-2 md:max-h-[38%] md:border-b md:border-border">
    {#if flat.length === 0}
      <div class="p-4 text-center text-[13px] muted">{$t('noFiles')}</div>
    {:else}
      {#each flat as node (node.path)}
        <button
          class="flex min-h-[40px] w-full items-center gap-2 rounded-sm px-2 text-left text-[12px] hover:bg-panel-2"
          class:bg-panel-2={selected === node.path}
          style={`padding-left:${8 + Math.min(node.depth, 8) * 12}px`}
          on:click={() => !isDir(node) && openFile(node.path)}
        >
          {#if isDir(node)}{#if query}<FolderOpen size={16} class="text-warn" />{:else}<Folder size={16} class="text-warn" />{/if}
          {:else}<File size={16} class="text-muted" />{/if}
          <span class="truncate mono">{node.path}</span>
        </button>
      {/each}
    {/if}
  </div>
  <div class="flex min-h-0 flex-1 flex-col">
    <div class="flex min-h-[40px] flex-none items-center gap-2 border-b border-border px-3">
      <File size={16} class="text-accent" />
      <span class="min-w-0 flex-1 truncate mono text-[12px]">{selected || $t('preview')}</span>
      <button class="btn btn-ghost btn-sm" aria-label={$t('copy')} on:click={copyFile}><Copy size={16} /></button>
    </div>
    <div class="relative min-h-0 flex-1 overflow-hidden">
      {#if loadingFile}
        <div class="skeleton m-3 h-4 w-2/3"></div>
        <div class="skeleton m-3 h-4 w-5/6"></div>
      {:else if selected}
        <div class="h-full w-full overflow-hidden" bind:this={host}></div>
        {#if !supported}<div class="absolute bottom-2 left-3 tiny muted">{$t('viewerUnavailable')}</div>{/if}
      {:else}
        <div class="flex h-full items-center justify-center text-[13px] muted">{$t('preview')}</div>
      {/if}
    </div>
  </div>
</div>
