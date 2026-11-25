export interface SharedPayload {
  text: string;
  title?: string;
  url?: string;
  files: File[];
}

const DB_NAME = 'ta-share';
const STORE = 'inbox';
const KEY = 'latest';

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, 1);
    req.onupgradeneeded = () => {
      if (!req.result.objectStoreNames.contains(STORE)) req.result.createObjectStore(STORE);
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error || new Error('IndexedDB unavailable'));
  });
}

export async function saveShared(payload: SharedPayload): Promise<void> {
  const db = await openDb();
  await new Promise<void>((resolve, reject) => {
    const tx = db.transaction(STORE, 'readwrite');
    tx.objectStore(STORE).put(payload, KEY);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error || new Error('Could not store share'));
  });
  db.close();
}

export async function takeShared(): Promise<SharedPayload | null> {
  try {
    const db = await openDb();
    const payload = await new Promise<SharedPayload | null>((resolve, reject) => {
      const tx = db.transaction(STORE, 'readwrite');
      const store = tx.objectStore(STORE);
      const get = store.get(KEY);
      get.onsuccess = () => resolve((get.result as SharedPayload) || null);
      get.onerror = () => reject(get.error || new Error('Could not read share'));
      store.delete(KEY);
    });
    db.close();
    return payload;
  } catch {
    return null;
  }
}
