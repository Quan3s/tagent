import type { UiEvent } from './types';

export interface ViewItem {
  key: string;
  type: string;
  role?: string;
  text?: string;
  ts?: number | string;
  id?: number;
  [key: string]: any;
}

/** Collapse streaming deltas into readable rows before rendering. */
export function buildViewItems(events: UiEvent[]): ViewItem[] {
  const items: ViewItem[] = [];
  const byTool = new Map<string, ViewItem>();

  for (const ev of events) {
    const type = ev.type;
    if (type === 'message.delta' || type === 'message') {
      const role = ev.role || 'assistant';
      const last = items[items.length - 1];
      if (last?.type === 'message' && last.role === role && (type === 'message.delta' || last.streaming)) {
        last.text = (last.text || '') + (ev.text || '');
        last.streaming = type === 'message.delta';
        last.ts = ev.ts ?? last.ts;
        last.id = ev.id ?? last.id;
      } else {
        items.push({ ...ev, type: 'message', role, text: ev.text || '', streaming: type === 'message.delta' });
      }
      continue;
    }
    if (type === 'thinking.delta' || type === 'thinking') {
      const last = items[items.length - 1];
      if (last?.type === 'thinking') {
        last.text = (last.text || '') + (ev.text || '');
        last.id = ev.id ?? last.id;
      } else {
        items.push({ ...ev, type: 'thinking', text: ev.text || '' });
      }
      continue;
    }
    if (type === 'tool.start') {
      const item: ViewItem = { ...ev, type: 'tool', status: 'running', args: ev.args ?? {}, name: ev.name || 'tool' };
      items.push(item);
      byTool.set(String(ev.tool_use_id), item);
      continue;
    }
    if (type === 'tool.result') {
      const existing = byTool.get(String(ev.tool_use_id));
      if (existing) {
        Object.assign(existing, {
          output: ev.output ?? '',
          is_error: !!ev.is_error,
          elapsed_ms: ev.elapsed_ms,
          status: ev.is_error ? 'error' : 'done',
        });
      } else {
        items.push({ ...ev, type: 'tool', status: ev.is_error ? 'error' : 'done' });
      }
      continue;
    }
    items.push({ ...ev });
  }
  return items;
}
