/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{svelte,js,ts}'],
  theme: {
    extend: {
      colors: {
        bg: 'var(--bg)',
        panel: 'var(--panel)',
        'panel-2': 'var(--panel-2)',
        border: 'var(--border)',
        'border-2': 'var(--border-2)',
        text: 'var(--text)',
        muted: 'var(--muted)',
        accent: 'var(--accent)',
        'accent-ink': 'var(--accent-ink)',
        warn: 'var(--warn)',
        danger: 'var(--danger)',
        'add-bg': 'var(--add-bg)',
        'add-fg': 'var(--add-fg)',
        'del-bg': 'var(--del-bg)',
        'del-fg': 'var(--del-fg)',
      },
      borderRadius: {
        sm: '8px',
        md: '12px',
        lg: '16px',
        pill: '999px',
      },
      fontFamily: {
        sans: ['Inter', 'system-ui', '-apple-system', 'Segoe UI', 'Roboto', 'sans-serif'],
        mono: ['JetBrains Mono', 'ui-monospace', 'SFMono-Regular', 'Menlo', 'monospace'],
      },
      fontSize: {
        ui: ['14px', '1.55'],
        code: ['12.5px', '1.55'],
      },
    },
  },
  plugins: [],
};
