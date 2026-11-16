/**
 * ホーム画面(index.html)で選んだ写真・動画を、写真のアプリ画面(app.html)・動画の画面(video.html)へ渡すための一時置き場。
 * 写真は KEY_PHOTOS、動画は KEY_VIDEO に分けて置くので、それぞれの画面は自分の分だけを取り出す。
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
const KEY_PHOTOS = 'files';
const KEY_VIDEO = 'video';
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
export function saveFiles(files) {
  return save(KEY_PHOTOS, files);
}

/** 保存されている写真を取り出して削除する。無ければ空配列（例外は投げない） */
export function takeFiles() {
  return take(KEY_PHOTOS);
}

/** 動画(1本)を保存する。保存できたら true */
export function saveVideo(file) {
  return save(KEY_VIDEO, [file]);
}

/** 保存されている動画を取り出して削除する。無ければ null */
export async function takeVideo() {
  const files = await take(KEY_VIDEO);
  return files[0] || null;
}

async function save(key, files) {
  try {
    await run('readwrite', (store) => store.put({ files: Array.from(files), savedAt: Date.now() }, key));
    return true;
  } catch {
    return false;
  }
}

async function take(key) {
  try {
    const entry = await run('readwrite', (store) => {
      const req = store.get(key);
      store.delete(key);
      return req;
    });
    if (!entry || Date.now() - entry.savedAt > MAX_AGE_MS) return [];
    return entry.files || [];
  } catch {
    return [];
  }
}
