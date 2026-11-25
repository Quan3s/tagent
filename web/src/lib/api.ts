import type { EventRecord, Extension, Provider, Session, TreeNode, UploadResult } from './types';

export class ApiError extends Error {
  status: number;
  constructor(message: string, status = 0) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
  }
}

/** The daemon hands out a short-lived token with its URL. Keep it for this tab only. */
export function getToken(): string {
  if (typeof window === 'undefined') return '';
  return sessionStorage.getItem('ta.token') || localStorage.getItem('ta.token') || '';
}

export function setToken(token: string): void {
  try {
    sessionStorage.setItem('ta.token', token);
  } catch {
    /* storage disabled */
  }
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  if (!headers.has('Accept')) headers.set('Accept', 'application/json');
  const res = await fetch(path, { ...init, headers, credentials: 'same-origin', cache: 'no-store' });
  if (!res.ok) {
    let message = `${res.status} ${res.statusText}`;
    try {
      const body = await res.json();
      message = body?.error || body?.message || message;
    } catch {
      const text = await res.text().catch(() => '');
      if (text) message = text.slice(0, 240);
    }
    throw new ApiError(message, res.status);
  }
  const type = res.headers.get('content-type') || '';
  if (!type.includes('json')) return (await res.text()) as unknown as T;
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

const json = (method: string, body?: unknown): RequestInit => ({
  method,
  headers: { 'Content-Type': 'application/json' },
  body: body === undefined ? undefined : JSON.stringify(body),
});

export const api = {
  health: () => request<{ ok: boolean }>('/api/health'),
  sessions: () => request<Session[]>('/api/sessions'),
  createSession: (title?: string) => request<Session>('/api/sessions', json('POST', title ? { title } : {})),
  events: (sessionId: string, lastEventId = 0) =>
    request<EventRecord[]>(`/api/sessions/${encodeURIComponent(sessionId)}/events?last_event_id=${lastEventId}`),

  tree: () => request<TreeNode[]>('/api/tree'),
  file: (path: string) => request<string>(`/api/files/${path.split('/').map(encodeURIComponent).join('/')}`),
  diff: () => request<{ diff: string }>('/api/diff'),
  diffAction: (action: 'apply' | 'reject' | 'undo') =>
    request<{ ok?: boolean }>(`/api/diff/${action}`, json('POST', {})),

  upload: (file: File) => {
    const form = new FormData();
    form.append('file', file, file.name);
    return request<UploadResult>('/api/uploads', { method: 'POST', body: form });
  },

  config: () => request<Record<string, any>>('/api/config'),
  saveConfig: (config: Record<string, any>) => request<Record<string, any>>('/api/config', json('PUT', config)),
  providers: () => request<Provider[]>('/api/providers'),
  auth: (provider: string, key: string) => request<{ provider: string; configured: boolean }>('/api/auth', json('POST', { provider, key })),

  extensions: (kind: 'plugins' | 'skills' | 'agents' | 'mcp') => request<Extension[]>(`/api/${kind}`),
  extensionAction: (kind: string, action: string, body: Record<string, unknown> = {}) =>
    request<any>(`/api/${kind}/${action}`, json('POST', body)),
};

export type StreamStatus = 'idle' | 'connecting' | 'open' | 'error';

/** One WebSocket per session, with an outbound queue and reconnect backoff. */
export class SessionStream {
  private ws: WebSocket | null = null;
  private queue: string[] = [];
  private timer: ReturnType<typeof setTimeout> | null = null;
  private retries = 0;
  private sessionId = '';
  private intentionalClose = false;

  onEvent: (event: EventRecord) => void = () => {};
  onStatus: (status: StreamStatus) => void = () => {};
  onError: (error: string) => void = () => {};

  get connected(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }

  connect(sessionId: string): void {
    if (sessionId === this.sessionId && this.ws) return;
    this.close(true);
    this.sessionId = sessionId;
    this.intentionalClose = false;
    this.open();
  }

  private open(): void {
    if (!this.sessionId) return;
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const token = getToken();
    const url = `${proto}//${location.host}/api/sessions/${encodeURIComponent(this.sessionId)}/stream${
      token ? `?token=${encodeURIComponent(token)}` : ''
    }`;
    this.onStatus('connecting');
    let ws: WebSocket;
    try {
      ws = new WebSocket(url);
    } catch (err) {
      this.onError(err instanceof Error ? err.message : String(err));
      this.onStatus('error');
      return;
    }
    this.ws = ws;
    ws.onopen = () => {
      this.retries = 0;
      this.onStatus('open');
      const pending = this.queue.splice(0);
      for (const msg of pending) ws.send(msg);
    };
    ws.onmessage = (ev) => {
      try {
        const data = JSON.parse(String(ev.data));
        if (Array.isArray(data)) data.forEach((item) => this.onEvent(item));
        else this.onEvent(data);
      } catch {
        // Ignore non-JSON frames (e.g. keep-alive pings).
      }
    };
    ws.onerror = () => this.onError('WebSocket connection failed');
    ws.onclose = () => {
      if (this.ws !== ws) return;
      this.ws = null;
      if (this.intentionalClose) return;
      this.onStatus('error');
      const delay = Math.min(8000, 500 * 2 ** this.retries++);
      this.timer = setTimeout(() => this.open(), delay);
    };
  }

  close(silent = false): void {
    this.intentionalClose = silent ? true : this.intentionalClose;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    if (this.ws) {
      const ws = this.ws;
      this.ws = null;
      ws.onclose = null;
      ws.close();
    }
    if (!silent) {
      this.intentionalClose = true;
      this.onStatus('idle');
    }
  }

  private send(message: Record<string, unknown>): void {
    const raw = JSON.stringify(message);
    if (this.connected) this.ws!.send(raw);
    else {
      this.queue.push(raw);
      if (!this.ws && this.sessionId) this.open();
    }
  }

  prompt(text: string, model: string, permissionMode: string, attachments: string[] = []): void {
    this.send({ type: 'prompt', text, model, permission_mode: permissionMode, attachments });
  }
  permissionReply(id: string, allow: boolean, always = false, edited = ''): void {
    this.send({ type: 'permission.reply', id, allow, always, edited });
  }
  interrupt(): void {
    this.send({ type: 'interrupt' });
  }
}
