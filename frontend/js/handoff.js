/**
 * ホーム画面(index.html)で選んだ写真を、アプリ画面(app.html)へ渡すための一時置き場。
 *
 * ページを移動すると File オブジェクトは消えてしまうので、ブラウザ内の IndexedDB に
 * いったん保存し、アプリ画面の起動時に取り出して(同時に削除して)アップロードに回す。
 * サーバーには何も送らない。
 *
 * - IndexedDB が使えない環境(一部のプライベートブラウズ等)では保存に失敗する。
 *   その場合 saveFiles は false を返すので、呼び出し側はアプリ画面で選び直してもらう。
 * - 取り出されなかった写真が残り続けないよう、保存から MAX_AGE_MS を過ぎたものは捨てる。
 */

const DB_NAME = 'gb-handoff';
const STORE = 'pending';
const KEY = 'files';
const MAX_AGE_MS = 5 * 60 * 1000;

function openDb() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

function run(mode, fn) {
  return openDb().then((db) => new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, mode);
    const result = fn(tx.objectStore(STORE));
    tx.oncomplete = () => { db.close(); resolve(result.result); };
    tx.onerror = () => { db.close(); reject(tx.error); };
    tx.onabort = () => { db.close(); reject(tx.error); };
  }));
}

/** 写真を保存する。保存できたら true、できなければ false（例外は投げない） */
export async function saveFiles(files) {
  try {
    await run('readwrite', (store) => store.put({ files: Array.from(files), savedAt: Date.now() }, KEY));
    return true;
  } catch {
    return false;
  }
}

/** 保存されている写真を取り出して削除する。無ければ空配列（例外は投げない） */
export async function takeFiles() {
  try {
    const entry = await run('readwrite', (store) => {
      const req = store.get(KEY);
      store.delete(KEY);
      return req;
    });
    if (!entry || Date.now() - entry.savedAt > MAX_AGE_MS) return [];
    return entry.files || [];
  } catch {
    return [];
  }
}
