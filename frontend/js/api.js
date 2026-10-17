import { getClientId } from './client-id.js';

// 同一オリジンで配信する想定。別ホストの場合はここを書き換える。
// （別ホストにする場合、バックエンドは CORS で `client-id` ヘッダーを許可する
//   必要がある。Access-Control-Allow-Headers に client-id を含めること。）
const BASE_URL = '';

/**
 * すべてのリクエストに付ける header を作る。
 * `client-id` はユーザー識別用のUUID（1ユーザー=1つ、リロードしても変わらない）。
 * 画像ごとのIDではない点に注意（js/client-id.js 参照）。
 */
function buildHeaders(extra = {}) {
  return { ...extra, 'client-id': getClientId() };
}


/**
 * HTTPステータスを、バックエンドの ErrorType に対応する種別と
 * 画面に出す文言に変換する。
 *
 * バックエンドのエラーレスポンスは { code } だけで message を持たないため、
 * 文言はフロント側で持つ。対応は presentation/errors/app_error.rs の
 *   InvalidInput → 400 / NotFound → 404 / Conflict → 409 / Internal → 500
 * に合わせている。
 */
const ERROR_MESSAGES = {
  BadRequest: '入力内容に問題があります。内容を確認してもう一度お試しください。',
  NotFound: '対象が見つかりません。時間が経って期限切れになった可能性があります。',
  Conflict: '処理が競合しました。少し待ってからもう一度お試しください。',
  Internal: 'サーバー内部でエラーが発生しました。時間をおいて再度お試しください。',
  Unknown: 'サーバーからエラーが返されました。',
};

function describeStatus(status) {
  let kind;
  if (status === 400) kind = 'BadRequest';
  else if (status === 404) kind = 'NotFound';
  else if (status === 409) kind = 'Conflict';
  else if (status >= 500) kind = 'Internal';
  else kind = 'Unknown';
  return { kind, message: ERROR_MESSAGES[kind] };
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
  const err = new Error(message);
  err.code = code;
  err.status = res.status;
  err.kind = kind;
  throw err;
}

async function parseJsonOrThrow(res) {
  if (!res.ok) {
    await throwApiError(res);
  }
  return res.json();
}

/**
 * POST /upload
 * request : multipart/form-data, field "file"
 * response: { session_id: string, image_id: string }
 */
export async function uploadImage(file) {
  const formData = new FormData();
  formData.append('file', file);

  const res = await fetch(`${BASE_URL}/upload`, {
    method: 'POST',
    // multipart の Content-Type は boundary 付きでブラウザが決めるので、ここでは指定しない
    headers: buildHeaders(),
    body: formData,
  });
  const data = await parseJsonOrThrow(res);
  return { sessionId: data.session_id, imageId: data.image_id };
}

/**
 * POST /get-preview
 * request : { image_id }
 * response: image/png バイナリ
 *
 * image_id はスナップショットのポインタ。segment/undo/redo のたびに
 * 新しい image_id が発行されるので、常に「直前の変更系リクエストが
 * 返した最新の image_id」で呼び出すこと。
 */
export async function getPreviewImage(imageId) {
  const res = await fetch(`${BASE_URL}/get-preview`, {
    method: 'POST',
    headers: buildHeaders({ 'Content-Type': 'application/json' }),
    body: JSON.stringify({ image_id: imageId }),
  });
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
  const res = await fetch(`${BASE_URL}/segment`, {
    method: 'POST',
    headers: buildHeaders({ 'Content-Type': 'application/json' }),
    body: JSON.stringify({
      session_id: sessionId,
      x: Math.round(point.x),
      y: Math.round(point.y),
      is_foreground: point.label === 1,
    }),
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
  const res = await fetch(`${BASE_URL}/undo`, {
    method: 'POST',
    headers: buildHeaders({ 'Content-Type': 'application/json' }),
    body: JSON.stringify({ session_id: sessionId }),
  });
  const data = await parseJsonOrThrow(res);
  return { imageId: data.image_id };
}

/**
 * POST /redo
 * request / response は /undo と同じ形。
 */
export async function redoEdit(sessionId) {
  const res = await fetch(`${BASE_URL}/redo`, {
    method: 'POST',
    headers: buildHeaders({ 'Content-Type': 'application/json' }),
    body: JSON.stringify({ session_id: sessionId }),
  });
  const data = await parseJsonOrThrow(res);
  return { imageId: data.image_id };
}

/**
 * POST /save
 * request : { session_id }
 * response: { image_id: string }  … フル画質版として生成された画像のID
 *
 * この image_id は編集プレビュー用の image_id とは別物（フル画質・保存専用の実体）。
 * 実体の取得は GET /get-completed で行う。
 */
export async function saveImage(sessionId) {
  const res = await fetch(`${BASE_URL}/save`, {
    method: 'POST',
    headers: buildHeaders({ 'Content-Type': 'application/json' }),
    body: JSON.stringify({ session_id: sessionId }),
  });
  const data = await parseJsonOrThrow(res);
  return { imageId: data.image_id };
}

/**
 * POST /get-completed
 * request : { image_id }
 * response: バイナリ（保存用フル画質画像）
 *
 * image_id は /save のレスポンスで返ってきたものをそのまま渡す。
 * 本来は参照系なのでGETが自然だが、image_id をJSONボディで渡す要件が
 * あるためPOSTにしている。**GETリクエストにJSONボディを持たせることは
 * fetch APIの仕様上できない**（`TypeError: Request with GET/HEAD method
 * cannot have body` で落ちる）ため、ここはPOST一択。
 */
export async function getCompletedImage(imageId) {
  const res = await fetch(`${BASE_URL}/get-completed`, {
    method: 'POST',
    headers: buildHeaders({ 'Content-Type': 'application/json' }),
    body: JSON.stringify({ image_id: imageId }),
  });
  if (!res.ok) {
    await throwApiError(res);
  }
  return res.blob();
}