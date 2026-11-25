export interface DiffLine {
  kind: 'add' | 'del' | 'ctx' | 'meta';
  text: string;
}
export interface DiffHunk {
  header: string;
  lines: DiffLine[];
}
export interface DiffFile {
  path: string;
  oldPath?: string;
  newPath?: string;
  added: number;
  removed: number;
  hunks: DiffHunk[];
}

export function parseDiff(raw: string): DiffFile[] {
  const lines = String(raw || '').replace(/\r\n?/g, '\n').split('\n');
  const files: DiffFile[] = [];
  let current: DiffFile | null = null;
  let hunk: DiffHunk | null = null;

  const newFile = (path: string): DiffFile => {
    const file: DiffFile = { path, added: 0, removed: 0, hunks: [] };
    files.push(file);
    return file;
  };

  for (const line of lines) {
    if (line.startsWith('diff --git ')) {
      const parts = line.match(/diff --git\s+"?a\/(.+?)"?\s+"?b\/(.+?)"?$/);
      current = newFile(parts?.[2] || parts?.[1] || 'unknown');
      if (parts?.[1]) current.oldPath = parts[1];
      if (parts?.[2]) current.newPath = parts[2];
      hunk = null;
      continue;
    }
    if (line.startsWith('+++ ')) {
      const path = line.slice(4).trim().replace(/^b\//, '');
      if (!current || current.path === 'unknown') current = newFile(path);
      else current.newPath = path;
      continue;
    }
    if (line.startsWith('--- ')) {
      if (current) current.oldPath = line.slice(4).trim().replace(/^a\//, '');
      continue;
    }
    if (line.startsWith('@@')) {
      if (!current) current = newFile('changes');
      hunk = { header: line, lines: [] };
      current.hunks.push(hunk);
      continue;
    }
    if (!current) continue;
    if (!hunk) {
      if (/^(new file|deleted file|index|similarity|rename|Binary)/.test(line) && current.hunks.length === 0) {
        // metadata, shown in the first hunk header when present
        current.path = current.path || 'changes';
      }
      continue;
    }
    if (line.startsWith('+')) {
      hunk.lines.push({ kind: 'add', text: line.slice(1) });
      current.added++;
    } else if (line.startsWith('-')) {
      hunk.lines.push({ kind: 'del', text: line.slice(1) });
      current.removed++;
    } else {
      hunk.lines.push({ kind: 'ctx', text: line.replace(/^[ \\\t]/, '') });
    }
  }
  return files;
}
