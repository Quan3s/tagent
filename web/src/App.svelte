<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import Sidebar from './components/Sidebar.svelte';
  import Header from './components/Header.svelte';
  import ChatStream from './components/ChatStream.svelte';
  import Composer from './components/Composer.svelte';
  import CommandPalette from './components/CommandPalette.svelte';
  import PermissionSheet from './components/PermissionSheet.svelte';
  import Panel from './components/Panel.svelte';
  import Toast from './components/Toast.svelte';
  import { api, SessionStream } from './lib/api';
  import { takeShared } from './lib/share';
  import {
    activeId,
    attachments,
    busy,
    closePanel,
    config,
    connectionError,
    diffText,
    drawer,
    events,
    extensionCounts,
    extensions,
    lastEventId,
    loading,
    makeEvent,
    mergeEvents,
    openPanel,
    palette,
    panel,
    permission,
    providers,
    sessions,
    shareDraft,
    streamStatus,
    todos,
    toast,
    tree,
    usage,
  } from './lib/store';
  import { t } from './lib/i18n';
  import type { EventRecord, Extension, PermissionMode, UploadResult } from './lib/types';
  import type { PanelType } from './lib/store';

  const stream = new SessionStream();
  let composer: Composer;
  let settingsTab = 'providers';
  let destroyed = false;

  stream.onEvent = handleEvent;
  stream.onStatus = (status) => {
    streamStatus.set(status);
    if (status === 'error') {
      connectionError.set($t('connectionLost'));
    } else if (status === 'open') {
      connectionError.set('');
    }
  };
  stream.onError = (message) => {
    connectionError.set(message);
  };

  onMount(async () => {
    window.addEventListener('keydown', globalKeydown);
    window.addEventListener('resize', onResize);
    void bootstrap();
    void consumeShareIntent();
  });

  onDestroy(() => {
    destroyed = true;
    stream.close();
    window.removeEventListener('keydown', globalKeydown);
    window.removeEventListener('resize', onResize);
  });

  async function bootstrap() {
    loading.set(true);
    const [sessionsResult] = await Promise.allSettled([
      api.sessions(),
      loadConfig(),
      loadProviders(),
      loadTree(),
      loadDiff(),
      loadExtensions(),
    ]);
    if (destroyed) return;
    if (sessionsResult.status === 'fulfilled') {
      sessions.set(sessionsResult.value);
      const active = sessionsResult.value[0];
      if (active) await selectSession(active.id);
    } else {
      toast($t('loadFailed'), 'error');
    }
    loading.set(false);
  }

  async function loadConfig() {
    try {
      config.set(await api.config());
    } catch {
      /* daemon may not expose config yet */
    }
  }
  async function loadProviders() {
    try {
      providers.set(await api.providers());
    } catch {
      /* optional endpoint */
    }
  }
  async function loadTree() {
    try {
      tree.set(await api.tree());
    } catch {
      /* optional endpoint */
    }
  }
  async function loadDiff() {
    try {
      diffText.set((await api.diff()).diff || '');
    } catch {
      /* optional endpoint */
    }
  }
  async function loadExtensions() {
    const kinds = ['plugins', 'skills', 'agents', 'mcp'] as const;
    const results = await Promise.allSettled(kinds.map((kind) => api.extensions(kind)));
    const next: Record<string, Extension[]> = {};
    kinds.forEach((kind, index) => {
      const result = results[index];
      next[kind] = result.status === 'fulfilled' ? result.value : [];
    });
    extensions.set(next);
  }

  async function selectSession(id: string) {
    activeId.set(id);
    events.set([]);
    permission.set(null);
    busy.set(false);
    loading.set(true);
    stream.connect(id);
    try {
      const history = await api.events(id, 0);
      if (get(activeId) !== id) return;
      mergeEvents(history.map((e) => makeEvent(e)));
    } catch {
      /* an empty or newly created session has no events yet */
    } finally {
      loading.set(false);
    }
  }

  function get<T>(store: { subscribe: (fn: (value: T) => void) => () => void }): T {
    let value!: T;
    const unsub = store.subscribe((v) => (value = v));
    unsub();
    return value;
  }

  async function newTask() {
    try {
      const session = await api.createSession();
      sessions.update((list) => [session, ...list.filter((s) => s.id !== session.id)]);
      await selectSession(session.id);
      closePanel();
      composer?.focus();
    } catch (err) {
      toast(err instanceof Error ? err.message : String(err), 'error');
    }
  }

  async function ensureSession(): Promise<string> {
    const current = get(activeId);
    if (current) return current;
    const session = await api.createSession();
    sessions.update((list) => [session, ...list.filter((s) => s.id !== session.id)]);
    await selectSession(session.id);
    return session.id;
  }

  async function send(payload: {
    text: string;
    model: string;
    permissionMode: string;
    attachments: string[];
  }) {
    try {
      await ensureSession();
      const id = lastEventId() + 0.1;
      mergeEvents([
        makeEvent({
          id,
          type: 'message.delta',
          role: 'user',
          text: payload.text,
          attachments: payload.attachments,
          ts: Date.now(),
        }),
      ]);
      sessions.update((list) =>
        list.map((s) => (s.id === get(activeId) ? { ...s, status: 'running', updated_at: Date.now() } : s)),
      );
      busy.set(true);
      stream.prompt(
        payload.text,
        payload.model,
        payload.permissionMode as PermissionMode,
        payload.attachments,
      );
    } catch (err) {
      toast(err instanceof Error ? err.message : String(err), 'error');
    }
  }

  function handleEvent(event: EventRecord) {
    if (!event || !event.type) return;
    const record = makeEvent(event);
    mergeEvents([record]);
    switch (event.type) {
      case 'usage':
        usage.update((u) => ({
          input_tokens: u.input_tokens + Number(event.input_tokens || 0),
          output_tokens: u.output_tokens + Number(event.output_tokens || 0),
          cost_usd: u.cost_usd + Number(event.cost_usd || 0),
        }));
        break;
      case 'diff':
        if (typeof event.diff === 'string') diffText.set(event.diff);
        break;
      case 'todo.update': {
        const items = event.items || event.todos || event.todo;
        if (Array.isArray(items)) todos.set(items);
        break;
      }
      case 'subagent.start':
      case 'subagent.result':
        break;
      case 'permission.request': {
        const request = {
          id: String(event.id || event.permission_id || record.key),
          tool: event.tool || 'tool',
          target: event.target || '',
          decision: event.decision || '',
        };
        permission.set(request);
        busy.set(true);
        break;
      }
      case 'permission.reply':
      case 'permission.resolved': {
        const resolvedId = String(event.id || event.permission_id || '');
        if (resolvedId) {
          events.update((list) =>
            list.map((e) =>
              String(e.id || e.permission_id) === resolvedId ? { ...e, answered: true } : e,
            ),
          );
        }
        permission.set(null);
        break;
      }
      case 'error':
        toast(String(event.message || $t('error')), 'error');
        busy.set(false);
        break;
      case 'done':
        busy.set(false);
        permission.set(null);
        sessions.update((list) =>
          list.map((s) => (s.id === get(activeId) ? { ...s, status: 'done', updated_at: Date.now() } : s)),
        );
        void api.sessions().then((list) => sessions.set(list)).catch(() => {});
        break;
    }
  }

  function replyPermission(id: string, allow: boolean, always: boolean, edited: string) {
    events.update((list) =>
      list.map((e) =>
        String(e.id || e.permission_id) === id || String(e.key) === id ? { ...e, answered: true, allow } : e,
      ),
    );
    permission.set(null);
    stream.permissionReply(id, allow, always, edited);
  }

  function interrupt() {
    stream.interrupt();
    busy.set(false);
    permission.set(null);
  }

  function openSettings(tab = 'providers') {
    settingsTab = tab;
    openPanel('settings');
  }

  function openPanelSafe(type: PanelType) {
    openPanel(type);
  }

  async function uploadDropped(files: FileList | File[] | null) {
    if (!files?.length) return;
    for (const file of Array.from(files)) {
      try {
        const result: UploadResult = await api.upload(file);
        attachments.update((list) => [...list, result]);
        composer?.insertToken(`@${result.path} `);
        toast(`${$t('uploaded')}: ${result.name}`, 'success');
      } catch (err) {
        toast(`${$t('uploadFailed')}: ${err instanceof Error ? err.message : String(err)}`, 'error');
      }
    }
  }

  function onDrop(event: DragEvent) {
    if (event.dataTransfer?.files?.length) {
      event.preventDefault();
      void uploadDropped(event.dataTransfer.files);
    }
  }

  async function consumeShareIntent() {
    const url = new URL(location.href);
    if (!url.searchParams.has('shared')) return;
    url.searchParams.delete('shared');
    history.replaceState(null, '', url.pathname + url.search + url.hash);
    const payload = await takeShared();
    if (payload) {
      shareDraft.set({ text: [payload.title, payload.text, payload.url].filter(Boolean).join('\n'), files: payload.files });
    }
  }

  function globalKeydown(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      palette.set({ open: true, query: '', kind: 'command' });
    } else if (event.key === 'Escape') {
      if (get(palette).open) palette.update((p) => ({ ...p, open: false }));
      else if (get(drawer)) drawer.set(false);
      else if (get(panel).open) closePanel();
      else if (get(busy)) interrupt();
    } else if (event.key === '?' && !/input|textarea|select/i.test((event.target as HTMLElement)?.tagName || '')) {
      event.preventDefault();
      palette.set({ open: true, query: '', kind: 'command' });
    }
  }

  function onResize() {
    if (window.innerWidth >= 760) drawer.set(false);
  }
</script>

<div class="app-shell" class:panel-open={$panel.open} on:dragover|preventDefault on:drop={onDrop} role="application">
  <div class="desktop-only h-full min-h-0">
    <Sidebar onNewTask={newTask} onSelect={selectSession} onOpenSettings={openSettings} />
  </div>

  <main class="flex h-full min-h-0 min-w-0 flex-col bg-bg">
    <Header onToggleDrawer={() => drawer.update((v) => !v)} onStop={interrupt} onOpenPanel={openPanelSafe} />
    {#if $connectionError}
      <div class="flex flex-none items-center gap-2 border-b border-warn/40 bg-warn/10 px-4 py-1 text-[11px] text-warn">
        {$connectionError} · {$t('reconnecting')}
      </div>
    {/if}
    <ChatStream
      onSuggestion={(text) => composer?.focus().then(() => composer.insertToken(text))}
      onPermissionReply={replyPermission}
      onOpenDiff={() => openPanel('diff')}
      activePermissionId={$permission?.id || ''}
    />
    <Composer bind:this={composer} onSend={send} onInterrupt={interrupt} />
  </main>

  {#if $panel.open}
    <div class="panel-wrap min-h-0">
      <Panel
        settingsTab={settingsTab}
        onProvidersChanged={loadProviders}
        onExtensionsChanged={loadExtensions}
      />
    </div>
  {/if}
</div>

{#if $drawer}
  <div class="overlay mobile-only" role="presentation" on:click={() => drawer.set(false)}></div>
  <div class="drawer mobile-only">
    <Sidebar onNewTask={newTask} onSelect={selectSession} onOpenSettings={openSettings} />
  </div>
{/if}

<CommandPalette onSelect={(token) => composer?.insertToken(token)} />
<PermissionSheet onReply={replyPermission} />
<Toast />
