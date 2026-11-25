/**
 * Lazily loaded CodeMirror 6. The specifier is kept in a variable with
 * @vite-ignore so the static build never hard-requires the packages; when
 * they are absent the caller renders a plain <textarea> instead.
 */
export interface EditorHandle {
  supported: boolean;
  getValue: () => string;
  setValue: (value: string) => void;
  destroy: () => void;
}

export async function createCodeEditor(host: HTMLElement, initial: string): Promise<EditorHandle> {
  try {
    const cmModule = 'codemirror';
    const stateModule = '@codemirror/state';
    const viewModule = '@codemirror/view';
    const [cm, state, view] = await Promise.all([
      import(/* @vite-ignore */ cmModule),
      import(/* @vite-ignore */ stateModule),
      import(/* @vite-ignore */ viewModule),
    ]);
    const basicSetup = (cm as any).basicSetup ?? [];
    const EditorState = (state as any).EditorState;
    const EditorView = (view as any).EditorView;
    if (!EditorState || !EditorView) throw new Error('CodeMirror unavailable');
    const editor = new EditorView({
      state: EditorState.create({
        doc: initial,
        extensions: [basicSetup, EditorView.editable.of(false)],
      }),
      parent: host,
    });
    return {
      supported: true,
      getValue: () => editor.state.doc.toString(),
      setValue: (value) => {
        editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: value } });
      },
      destroy: () => editor.destroy(),
    };
  } catch {
    const area = document.createElement('textarea');
    area.value = initial;
    area.readOnly = true;
    area.spellcheck = false;
    area.className = 'lazy-editor-fallback';
    host.appendChild(area);
    return {
      supported: false,
      getValue: () => area.value,
      setValue: (value) => (area.value = value),
      destroy: () => area.remove(),
    };
  }
}
