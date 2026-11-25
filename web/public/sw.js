/* ta PWA service worker: app-shell offline cache + Android share target handoff. */
const VERSION = 'ta-web-v1';
const SHELL = ['/', '/index.html', '/manifest.webmanifest', '/icon.svg', '/icon-192.png', '/icon-512.png'];

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches
      .open(VERSION)
      .then((cache) => cache.addAll(SHELL))
      .then(() => self.skipWaiting())
      .catch(() => self.skipWaiting()),
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((key) => key !== VERSION).map((key) => caches.delete(key))))
      .then(() => self.clients.claim()),
  );
});

function openDb() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open('ta-share', 1);
    req.onupgradeneeded = () => {
      if (!req.result.objectStoreNames.contains('inbox')) req.result.createObjectStore('inbox');
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function stashShare(form) {
  const parts = [form.get('title'), form.get('text'), form.get('url')]
    .map((value) => (typeof value === 'string' ? value.trim() : ''))
    .filter(Boolean);
  const files = form.getAll('files').filter((value) => typeof value === 'object');
  const db = await openDb();
  await new Promise((resolve, reject) => {
    const tx = db.transaction('inbox', 'readwrite');
    tx.objectStore('inbox').put({ text: parts.join('\n'), files }, 'latest');
    tx.oncomplete = resolve;
    tx.onerror = () => reject(tx.error);
  });
  db.close();
}

async function handleShare(request) {
  try {
    const form = await request.formData();
    await stashShare(form);
    return Response.redirect('/?shared=1', 303);
  } catch (error) {
    return new Response(`Share failed: ${error}`, { status: 400, headers: { 'Content-Type': 'text/plain' } });
  }
}

self.addEventListener('fetch', (event) => {
  const request = event.request;
  const url = new URL(request.url);

  if (request.method === 'POST' && url.origin === self.location.origin && url.pathname === '/') {
    event.respondWith(handleShare(request));
    return;
  }
  if (request.method !== 'GET' || url.origin !== self.location.origin || url.pathname.startsWith('/api/')) return;

  if (request.mode === 'navigate') {
    event.respondWith(
      fetch(request)
        .then((response) => {
          if (response.ok) {
            const copy = response.clone();
            void caches.open(VERSION).then((cache) => cache.put('/index.html', copy));
          }
          return response;
        })
        .catch(async () => (await caches.match('/index.html')) || (await caches.match('/')) || Response.error()),
    );
    return;
  }

  event.respondWith(
    caches.match(request).then(
      (cached) =>
        cached ||
        fetch(request).then((response) => {
          if (response.ok && (url.pathname.startsWith('/assets/') || /\.(js|css|png|svg|woff2?|webmanifest)$/.test(url.pathname))) {
            const copy = response.clone();
            void caches.open(VERSION).then((cache) => cache.put(request, copy));
          }
          return response;
        }),
    ),
  );
});
