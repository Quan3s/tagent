export interface Session {
  id: string;
  title: string;
  updated_at?: string | number;
  status?: 'running' | 'waiting' | 'done' | 'idle' | 'error';
  repo?: string;
  branch?: string;
  [key: string]: unknown;
}

export interface EventRecord {
  id?: number;
  ts?: number | string;
  type: string;
  [key: string]: any;
}

export type UiEvent = EventRecord & { key: string; local?: boolean };

export interface TreeNode {
  path: string;
  type: 'file' | 'dir' | 'directory' | string;
  [key: string]: unknown;
}

export interface UploadResult {
  file_id: string;
  path: string;
  name: string;
  size: number;
}

export interface Provider {
  name: string;
  type: string;
  configured: boolean;
  models?: string[];
  [key: string]: unknown;
}

export interface Extension {
  name: string;
  enabled?: boolean;
  description?: string;
  version?: string;
  [key: string]: unknown;
}

export interface Toast {
  id: number;
  kind: 'error' | 'success' | 'info';
  message: string;
}

export type Theme = 'dark' | 'light' | 'amoled';
export type Lang = 'vi' | 'en';
export type PermissionMode = 'ask' | 'auto-edit' | 'plan' | 'yolo';

export interface PermissionRequest {
  id: string;
  tool: string;
  target: string;
  decision?: string;
  edited?: string;
  resolved?: boolean;
  allow?: boolean;
}
