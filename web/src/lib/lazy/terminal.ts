/** Lazily loaded xterm.js with a plain <pre> fallback. */
export interface TerminalHandle {
  supported: boolean;
  write: (data: string) => void;
  dispose: () => void;
}

export async function createTerminal(host: HTMLElement): Promise<TerminalHandle> {
  try {
    const specifier = '@xterm/xterm';
    const mod: any = await import(/* @vite-ignore */ specifier);
    const Xterm = mod.Terminal || mod.default;
    if (typeof Xterm !== 'function') throw new Error('xterm unavailable');
    const term = new Xterm({
      convertEol: true,
      fontSize: 12,
      fontFamily: 'JetBrains Mono, ui-monospace, monospace',
      cursorBlink: false,
      theme: { background: '#0e0f11', foreground: '#e8e9ec' },
    });
    term.open(host);
    return {
      supported: true,
      write: (data) => term.write(data),
      dispose: () => term.dispose(),
    };
  } catch {
    const pre = document.createElement('pre');
    pre.className = 'lazy-terminal-fallback';
    pre.textContent = '';
    host.appendChild(pre);
    return {
      supported: false,
      write: (data) => {
        pre.textContent += data;
        pre.scrollTop = pre.scrollHeight;
      },
      dispose: () => pre.remove(),
    };
  }
}
