import { derived, get, writable, type Writable } from 'svelte/store';
import type {
  Extension,
  Lang,
  PermissionMode,
  PermissionRequest,
  Session,
  Theme,
  Toast,
  TreeNode,
  UiEvent,
  UploadResult,
} from './types';
import type { StreamStatus } from './api';

function persisted<T>(key: string, fallback: T): Writable<T> {
  let initial = fallback;
  try {
    const raw = localStorage.getItem(`ta.${key}`);
    if (raw !== null) initial = JSON.parse(raw) as T;
  } catch {
    /* ignore malformed or unavailable storage */
  }
  const store = writable<T>(initial);
  store.subscribe((value) => {
    try {
      localStorage.setItem(`ta.${key}`, JSON.stringify(value));
    } catch {
      /* storage may be disabled */
    }
  });
  return store;
}

export const sessions = writable<Session[]>([]);
export const activeId = writable<string>('');
export const activeSession = derived([sessions, activeId], ([$sessions, $activeId]) =>
  $sessions.find((s) => s.id === $activeId) || null,
);
export const events = writable<UiEvent[]>([]);
export const streamStatus = writable<StreamStatus>('idle');
export const busy = writable(false);
export const loading = writable(true);
export const connectionError = writable('');

export const config = writable<Record<string, any>>({});
export const providers = writable<{ name: string; type: string; configured: boolean; models?: string[] }[]>([]);
export const extensions = writable<Record<string, Extension[]>>({ plugins: [], skills: [], agents: [], mcp: [] });
export const extensionCounts = derived(extensions, ($e) => ({
  plugins: $e.plugins?.length || 0,
  skills: $e.skills?.length || 0,
  agents: $e.agents?.length || 0,
  mcp: $e.mcp?.length || 0,
}));
export const tree = writable<TreeNode[]>([]);
export const diffText = writable('');
export const todos = writable<any[]>([]);
export const subagents = writable<any[]>([]);
export const usage = writable({ input_tokens: 0, output_tokens: 0, cost_usd: 0 });
export const attachments = writable<UploadResult[]>([]);
export const permission = writable<PermissionRequest | null>(null);

export const theme = persisted<Theme>('theme', 'dark');
export const lang = persisted<Lang>('lang', 'vi');
export const permissionMode = persisted<PermissionMode>('permissionMode', 'ask');
export const model = persisted<string>('model', '');

export type PanelType = 'files' | 'diff' | 'terminal' | 'todos' | 'agents' | 'settings' | null;
export interface PanelState {
  type: PanelType;
  open: boolean;
  filePath?: string;
}
export const panel = writable<PanelState>({ type: null, open: false });
export const drawer = writable(false);
export const palette = writable<{ open: boolean; query: string; kind: 'command' | 'file' }>({
  open: false,
  query: '',
  kind: 'command',
});
export const toasts = writable<Toast[]>([]);

export const shareDraft = writable<{ text: string; files: File[] }>({ text: '', files: [] });

let toastId = 0;
export function toast(message: string, kind: Toast['kind'] = 'info'): void {
  const id = ++toastId;
  toasts.update((list) => [...list, { id, kind, message }]);
  setTimeout(() => dismissToast(id), 5200);
}
export function dismissToast(id: number): void {
  toasts.update((list) => list.filter((t) => t.id !== id));
}

export function applyTheme(value: Theme): void {
  if (typeof document === 'undefined') return;
  document.documentElement.setAttribute('data-theme', value);
  const meta = document.querySelector<HTMLMetaElement>('meta[name="theme-color"]');
  if (meta) meta.content = value === 'light' ? '#f6f7f9' : value === 'amoled' ? '#000000' : '#0e0f11';
  const scheme = document.querySelector<HTMLMetaElement>('meta[name="color-scheme"]');
  if (scheme) scheme.content = value === 'light' ? 'light' : 'dark';
}
theme.subscribe(applyTheme);
lang.subscribe((value) => {
  if (typeof document !== 'undefined') document.documentElement.lang = value;
});

let eventSeq = 0;
export function makeEvent(record: Record<string, any>): UiEvent {
  return {
    ...record,
    type: record.type || 'unknown',
    key: String(record.id ?? `${record.type || 'event'}-${++eventSeq}`),
  } as UiEvent;
}

export function mergeEvents(incoming: UiEvent[]): void {
  events.update((current) => {
    const byId = new Map<string, UiEvent>();
    for (const ev of [...current, ...incoming]) {
      const key = ev.key;
      if (byId.has(key)) {
        const prev = byId.get(key)!;
        byId.set(key, { ...prev, ...ev, text: (prev.text || '') + (ev.type === 'message.delta' || ev.type === 'thinking.delta' ? '' : (ev.text || '')) });
      } else byId.set(key, ev);
    }
    return [...byId.values()].sort((a, b) => (a.id ?? 0) - (b.id ?? 0));
  });
}

export function lastEventId(): number {
  const ids = get(events).map((e) => e.id || 0);
  return ids.length ? Math.max(...ids) : 0;
}

export function openPanel(type: PanelType, filePath?: string): void {
  panel.set({ type, open: true, filePath });
}
export function closePanel(): void {
  panel.set({ type: null, open: false });
}

/** Resolve the model to send, preferring the user's chip, then config roles. */
export function currentModel(): string {
  const chosen = get(model);
  if (chosen) return chosen;
  const roles = get(config)?.roles;
  if (roles?.main && typeof roles.main === 'string' && roles.main.includes('/')) return roles.main;
  const ps = get(providers);
  if (ps.length) {
    const configured = ps.find((p) => p.configured) || ps[0];
    const first = configured.models?.[0];
    return first && first.includes('/') ? first : `${configured.name}/default`;
  }
  return 'default';
}
