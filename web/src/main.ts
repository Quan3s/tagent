import './app.css';
import App from './App.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('Missing #app root');

// The daemon prints a URL with `?token=…`. Exchange it for an HttpOnly cookie
// and remove it from history so the token is not left in the address bar.
const params = new URLSearchParams(window.location.search);
const token = params.get('token');
if (token) {
  sessionStorage.setItem('ta.token', token);
  params.delete('token');
  const clean = window.location.pathname + (params.toString() ? `?${params}` : '') + window.location.hash;
  history.replaceState(null, '', clean);
  void fetch('/api/login', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'same-origin',
    body: JSON.stringify({ token }),
  }).catch(() => {
    // The token is still in sessionStorage; WebSocket auth will work.
  });
}

const app = new App({ target });

if ('serviceWorker' in navigator && import.meta.env.PROD) {
  window.addEventListener('load', () => {
    navigator.serviceWorker.register('/sw.js').catch(() => {
      // Offline support is best-effort; the app works without it.
    });
  });
}

export default app;
