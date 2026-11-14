/**
 * アップロードした写真と編集の状態を、リロードしても消えないようにブラウザ内(IndexedDB)に保存する。
 *
 * 保存するもの（写真1枚ごと）:
 *   - files    ストア: 元の写真ファイル（Blob）。最初に1回だけ書く
 *   - sessions ストア: サーバー側の session_id / image_id、タップした点、状態、最後にサーバーで触った時刻
 *   - meta     ストア: 開いていた写真（activeLocalId）
 *
 * サーバー側の期限（重要）:
 *   サーバーは画像と編集状態を「最後にアクセスしてから10分」で消す（config.rs の TTL_SEC）。
 *   編集状態と元画像の寿命が延びるのは upload / segment / undo / redo / save のときなので、
 *   その時刻を touchedAt として持ち、そこから10分(少し余裕を引く)を過ぎていたら期限切れとみなす。
 *   期限切れの写真は、開いたときに保存しておいた元ファイルで自動的に送り直す（editor.js）。
 *   サーバーの再起動などで期限内でも消えていることがあるので、404 が返ったときも同じ扱いにする。
 *
 * ブラウザ内に写真を残し続けないよう、最後にサーバーで触ってから KEEP_MS を過ぎたものは起動時に消す。
 * IndexedDB が使えない環境では何も保存しない（今までどおりリロードで消えるだけで、動作は止めない）。
 */

const DB_NAME = 'gb-sessions';
const FILES = 'files';
const SESSIONS = 'sessions';
const META = 'meta';

/** サーバー側の期限（config.rs の TTL_SEC と同じ値にすること） */
export const SERVER_TTL_MS = 10 * 60 * 1000;
/** 期限ぎりぎりの写真を「まだ使える」と判断して失敗しないよう、少し早めに期限切れとみなす */
const EXPIRY_MARGIN_MS = 30 * 1000;
/** ブラウザ内に写真を残しておく期間 */
const KEEP_MS = 24 * 60 * 60 * 1000;
/** 編集のたびに書き込みが集中しないよう、まとめて書くまでの待ち時間 */
const SAVE_DELAY_MS = 300;

/** サーバー側で期限切れになっていそうか */
export function isLikelyExpired(touchedAt) {
  return !touchedAt || Date.now() - touchedAt > SERVER_TTL_MS - EXPIRY_MARGIN_MS;
}

let dbPromise = null;
function openDb() {
  if (!dbPromise) {
    dbPromise = new Promise((resolve, reject) => {
      const req = indexedDB.open(DB_NAME, 1);
      req.onupgradeneeded = () => {
        const db = req.result;
        db.createObjectStore(FILES);
        db.createObjectStore(SESSIONS);
        db.createObjectStore(META);
      };
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
    });
  }
  return dbPromise;
}

function tx(storeNames, mode, fn) {
  return openDb().then((db) => new Promise((resolve, reject) => {
    const t = db.transaction(storeNames, mode);
    const stores = Object.fromEntries(storeNames.map((n) => [n, t.objectStore(n)]));
    const result = fn(stores);
    t.oncomplete = () => resolve(result);
    t.onerror = () => reject(t.error);
    t.onabort = () => reject(t.error);
  }));
}

/**
 * 保存されている写真を読み出す。古すぎるもの・ファイルが欠けているものはここで消す。
 * @returns {Promise<{ records: Array<object & { file: Blob }>, activeLocalId: string|null }>}
 */
export async function loadSaved() {
  try {
    // 3つのストアを1つのトランザクションで読む。結果は各リクエストの onsuccess で out に入れ、
    // トランザクション完了(=全リクエスト完了)後に使う
    const { metas, files, active } = await tx([SESSIONS, FILES, META], 'readonly', (s) => {
      const out = { metas: [], files: new Map(), active: null };
      const metasReq = s[SESSIONS].getAll();
      metasReq.onsuccess = () => { out.metas = metasReq.result; };
      const cursorReq = s[FILES].openCursor();
      cursorReq.onsuccess = () => {
        const c = cursorReq.result;
        if (c) { out.files.set(c.key, c.value); c.continue(); }
      };
      const activeReq = s[META].get('activeLocalId');
      activeReq.onsuccess = () => { out.active = activeReq.result ?? null; };
      return out;
    });

    const now = Date.now();
    const keep = [];
    const drop = [];
    for (const meta of metas || []) {
      const file = files.get(meta.localId);
      const lastUsed = meta.touchedAt || meta.createdAt || 0;
      if (!file || now - lastUsed > KEEP_MS) drop.push(meta.localId);
      else keep.push({ ...meta, file });
    }
    if (drop.length > 0) {
      await tx([SESSIONS, FILES], 'readwrite', (s) => {
        for (const id of drop) { s[SESSIONS].delete(id); s[FILES].delete(id); }
      });
    }
    keep.sort((a, b) => a.order - b.order);
    for (const rec of keep) { written.set(rec.localId, signature(rec)); writtenFiles.add(rec.localId); }
    return { records: keep, activeLocalId: keep.some((r) => r.localId === active) ? active : null };
  } catch {
    return { records: [], activeLocalId: null };
  }
}

// ---- 自動保存 ----

const written = new Map(); // localId -> 最後に書いた内容(JSON)。変わったものだけ書く
const writtenFiles = new Set(); // ファイルを書き込み済みの localId
let lastActive;

// 保存する項目だけを取り出す（Blob URL や busy などその場限りの値は保存しない）
function toRecord(session, order) {
  return {
    localId: session.localId,
    name: session.file ? session.file.name : '画像',
    sessionId: session.sessionId,
    imageId: session.imageId,
    points: session.points,
    redoStack: session.redoStack,
    // 保存中・アップロード中に閉じられた場合は、次に開いたとき送り直し/やり直しができる状態に戻す
    status: session.status === 'saving' ? 'ready' : session.status,
    touchedAt: session.touchedAt || null,
    createdAt: session.createdAt || Date.now(),
    order,
  };
}

function signature(rec) {
  const { file, ...rest } = rec;
  return JSON.stringify(rest);
}

/** store の変化を見て、変わった写真だけを IndexedDB に書く */
export function startAutoSave(store) {
  let timer = null;
  store.subscribe(() => {
    clearTimeout(timer);
    timer = setTimeout(() => flush(store.getState()), SAVE_DELAY_MS);
  });
  // ページを閉じる直前の変更も取りこぼさない
  window.addEventListener('pagehide', () => flush(store.getState()));
}

async function flush(state) {
  const puts = [];
  const filePuts = [];
  const current = new Set();
  let order = 0;
  for (const session of state.sessions.values()) {
    order += 1;
    // 失敗した写真は一覧から消してもらう前提なので残さない。ファイルが無いものも残せない
    if (session.status === 'error' || !session.file) continue;
    current.add(session.localId);
    const rec = toRecord(session, order);
    const sig = signature(rec);
    if (written.get(session.localId) !== sig) puts.push([rec, sig]);
    if (!writtenFiles.has(session.localId)) filePuts.push(session);
  }
  const deletes = [...written.keys()].filter((id) => !current.has(id));
  const active = state.activeLocalId;
  if (puts.length === 0 && filePuts.length === 0 && deletes.length === 0 && active === lastActive) return;

  try {
    await tx([SESSIONS, FILES, META], 'readwrite', (s) => {
      for (const [rec] of puts) s[SESSIONS].put(rec, rec.localId);
      for (const session of filePuts) s[FILES].put(session.file, session.localId);
      for (const id of deletes) { s[SESSIONS].delete(id); s[FILES].delete(id); }
      s[META].put(active, 'activeLocalId');
    });
    for (const [rec, sig] of puts) written.set(rec.localId, sig);
    for (const session of filePuts) writtenFiles.add(session.localId);
    for (const id of deletes) { written.delete(id); writtenFiles.delete(id); }
    lastActive = active;
  } catch {
    /* 保存できなくても編集は続けられる（リロードで消えるだけ） */
  }
}
