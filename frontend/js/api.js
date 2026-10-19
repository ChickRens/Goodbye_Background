import { getClientId } from './client-id.js';

// 同一オリジンで配信する想定。別ホストの場合はここを書き換える。
// （別ホストにする場合、バックエンドは CORS で `client-id` ヘッダーを許可する
//   必要がある。Access-Control-Allow-Headers に client-id を含めること。）
const BASE_URL = '';

// サーバーが応答しないまま待たされ続けないよう、リクエストにはタイムアウトを付ける。
// アップロードと保存(フル画質生成)は重いので長め。
const DEFAULT_TIMEOUT_MS = 60_000;
const LONG_TIMEOUT_MS = 180_000;

/**
 * すべてのリクエストに付ける header を作る。
 * `client-id` はユーザー識別用のUUID（1ユーザー=1つ、リロードしても変わらない）。
 * 画像ごとのIDではない点に注意（js/client-id.js 参照）。
 */
function buildHeaders(extra = {}) {
  return { ...extra, 'client-id': getClientId() };
}

/**
 * HTTPステータスや通信失敗を、種別と画面に出す文言に変換する。
 *
 * バックエンドのエラーレスポンスは { code } だけで message を持たないため、
 * 文言はフロント側で持つ。対応は presentation/errors/app_error.rs の
 *   InvalidInput → 400 / NotFound → 404 / Conflict → 409 / Internal → 500
 * に合わせている。413/429 はサーバー前段（Cloud Run 等）が返し得るもの。
 * Network / Timeout は fetch 自体が失敗した場合（HTTPステータスは無い）。
 */
const ERROR_MESSAGES = {
  BadRequest: '入力内容に問題があります。内容を確認してもう一度お試しください。',
  NotFound: '対象が見つかりません。時間が経って期限切れになった可能性があります。',
  Conflict: '処理が競合しました。少し待ってからもう一度お試しください。',
  PayloadTooLarge: '画像のサイズが大きすぎます。小さい画像でお試しください。',
  TooManyRequests: 'リクエストが多すぎます。少し待ってからもう一度お試しください。',
  Internal: 'サーバー内部でエラーが発生しました。時間をおいて再度お試しください。',
  Network: '通信に失敗しました。ネットワーク接続を確認してもう一度お試しください。',
  Timeout: 'サーバーからの応答がありませんでした。時間をおいて再度お試しください。',
  Unknown: 'サーバーからエラーが返されました。',
};

function describeStatus(status) {
  let kind;
  if (status === 400) kind = 'BadRequest';
  else if (status === 404) kind = 'NotFound';
  else if (status === 409) kind = 'Conflict';
  else if (status === 413) kind = 'PayloadTooLarge';
  else if (status === 429) kind = 'TooManyRequests';
  else if (status >= 500) kind = 'Internal';
  else kind = 'Unknown';
  return { kind, message: ERROR_MESSAGES[kind] };
}

function createError(kind, message, { code = null, status = 0 } = {}) {
  const err = new Error(message);
  err.code = code;
  err.status = status;
  err.kind = kind;
  return err;
}

/**
 * fetch にタイムアウトと、通信失敗時の日本語エラーを付けたもの。
 * （タイマーはレスポンスヘッダーを受け取った時点で解除する。ボディの読み込みは対象外）
 */
async function request(path, options, timeoutMs = DEFAULT_TIMEOUT_MS) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    return await fetch(`${BASE_URL}${path}`, { ...options, signal: controller.signal });
  } catch (cause) {
    const kind = cause && cause.name === 'AbortError' ? 'Timeout' : 'Network';
    console.error(`[API error] ${kind} (${path})`, cause);
    throw createError(kind, ERROR_MESSAGES[kind]);
  } finally {
    clearTimeout(timer);
  }
}

/**
 * エラーレスポンスのJSONボディから一意のエラーコード(code)を取り出し、
 * デバッグ用にコンソールへ出力してから例外を投げる。
 * 呼び出し側で code を使いたい場合は err.code から取得できる。
 */
async function throwApiError(res) {
  let code = null;
  try {
    const body = await res.json();
    if (body && body.code) code = body.code;
  } catch {
    /* JSONでないエラーレスポンスはHTTPステータスのみで扱う */
  }

  if (code) {
    console.error(`[API error] code=${code} (HTTP ${res.status})`);
  } else {
    console.error(`[API error] code不明 (HTTP ${res.status})`);
  }

  const { kind, message } = describeStatus(res.status);
  throw createError(kind, message, { code, status: res.status });
}

async function parseJsonOrThrow(res) {
  if (!res.ok) {
    await throwApiError(res);
  }
  return res.json();
}

function postJson(path, body, timeoutMs) {
  return request(
    path,
    {
      method: 'POST',
      headers: buildHeaders({ 'Content-Type': 'application/json' }),
      body: JSON.stringify(body),
    },
    timeoutMs,
  );
}

/**
 * POST /upload
 * request : multipart/form-data, field "file"
 * response: { session_id: string, image_id: string }
 */
export async function uploadImage(file) {
  const formData = new FormData();
  formData.append('file', file);

  const res = await request(
    '/upload',
    {
      method: 'POST',
      // multipart の Content-Type は boundary 付きでブラウザが決めるので、ここでは指定しない
      headers: buildHeaders(),
      body: formData,
    },
    LONG_TIMEOUT_MS,
  );
  const data = await parseJsonOrThrow(res);
  return { sessionId: data.session_id, imageId: data.image_id };
}

/**
 * POST /get-preview
 * request : { image_id }
 * response: image/webp バイナリ（バックエンドの get_preview/handler.rs に合わせる）
 *
 * image_id はスナップショットのポインタ。segment/undo/redo のたびに
 * 新しい image_id が発行されるので、常に「直前の変更系リクエストが
 * 返した最新の image_id」で呼び出すこと。
 *
 * 戻り値は Blob URL。使い終わったら URL.revokeObjectURL() で解放すること
 * （解放しないと操作のたびに画像がメモリに溜まる）。
 */
export async function getPreviewImage(imageId) {
  const res = await postJson('/get-preview', { image_id: imageId });
  if (!res.ok) {
    await throwApiError(res);
  }
  const blob = await res.blob();
  return URL.createObjectURL(blob);
}

/**
 * POST /segment
 * request : { session_id, x, y, is_foreground: boolean }
 *   - x, y は整数（Rust側 u16 相当）。canvas上の座標計算は浮動小数点に
 *     なるため、送信前に必ず丸める。u16なので0未満・65535超も弾かれる点に注意。
 *   - is_foreground: true = 含める(positive), false = 除外する(negative)
 *   - 1回の呼び出しにつき1点だけ送る（累積したpoints配列は送らない。
 *     累積の管理はサーバー側がsession_idに紐づけて持っている前提）
 * response: { image_id: string }
 */
export async function segmentImage(sessionId, point) {
  const res = await postJson('/segment', {
    session_id: sessionId,
    x: Math.round(point.x),
    y: Math.round(point.y),
    is_foreground: point.label === 1,
  });
  const data = await parseJsonOrThrow(res);
  return { imageId: data.image_id };
}

/**
 * POST /undo
 * request : { session_id }
 * response: { image_id: string }
 */
export async function undoEdit(sessionId) {
  const res = await postJson('/undo', { session_id: sessionId });
  const data = await parseJsonOrThrow(res);
  return { imageId: data.image_id };
}

/**
 * POST /redo
 * request / response は /undo と同じ形。
 */
export async function redoEdit(sessionId) {
  const res = await postJson('/redo', { session_id: sessionId });
  const data = await parseJsonOrThrow(res);
  return { imageId: data.image_id };
}

/**
 * POST /save
 * request : { session_id }
 * response: { image_id: string }  … フル画質版として生成された画像のID
 *
 * この image_id は編集プレビュー用の image_id とは別物（フル画質・保存専用の実体）。
 * 実体の取得は POST /get-completed で行う。
 */
export async function saveImage(sessionId) {
  const res = await postJson('/save', { session_id: sessionId }, LONG_TIMEOUT_MS);
  const data = await parseJsonOrThrow(res);
  return { imageId: data.image_id };
}

/**
 * POST /get-completed
 * request : { image_id }
 * response: バイナリ（保存用フル画質画像。バックエンドは image/webp で返す）
 *
 * image_id は /save のレスポンスで返ってきたものをそのまま渡す。
 * 本来は参照系なのでGETが自然だが、image_id をJSONボディで渡す要件が
 * あるためPOSTにしている。**GETリクエストにJSONボディを持たせることは
 * fetch APIの仕様上できない**（`TypeError: Request with GET/HEAD method
 * cannot have body` で落ちる）ため、ここはPOST一択。
 *
 * 戻り値の Blob は blob.type に実際の形式（image/webp 等）が入る。
 * ファイル拡張子はそれに合わせること（editor.js の triggerDownload）。
 */
export async function getCompletedImage(imageId) {
  const res = await postJson('/get-completed', { image_id: imageId }, LONG_TIMEOUT_MS);
  if (!res.ok) {
    await throwApiError(res);
  }
  return res.blob();
}