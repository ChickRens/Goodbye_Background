/**
 * 動画ページで出す日本語の文言。
 * サーバーは失敗コード・エラーコードだけを返すので、文言はここで持つ（仕様 §5.1）。
 */

/** GET /video-jobs/{id} が 404 のとき（サーバーの再起動でジョブが消えた） */
export const SERVER_RESTARTED_MESSAGE = 'サーバーが再起動したため、やり直してください';

/** ジョブの失敗コード（failure_code）→ 文言 */
const FAILURE_MESSAGES = {
  dispatch_failed: '混み合っています。時間をおいて試してください',
  timeout: '処理が時間内に終わりませんでした。時間をおいて試してください',
  internal_error: 'サーバーでエラーが起きました。時間をおいて試してください',
  subject_not_found: '最初のフレームで被写体が見つかりませんでした。点を打ち直して試してください',
  unsupported_video: 'この動画はサーバーで読み込めませんでした。MP4（H.264）で試してください',
  range_out_of_bounds: '選んだ範囲が動画の長さを超えています。範囲を選び直してください',
  too_large: '動画が大きすぎます（200MB まで）',
};

export function failureMessage(code) {
  return FAILURE_MESSAGES[code] || '処理に失敗しました。時間をおいて試してください';
}

/** 失敗したとき、今日の回数が戻ったかどうか */
export function refundMessage(quotaRefunded) {
  return quotaRefunded ? '回数は戻りました' : '回数は戻りません';
}

/** 処理中の状態（phase）→ 文言。progress は 0〜1（tracking のときだけ来る） */
export function phaseMessage(phase, progress) {
  switch (phase) {
    case 'created':
      return '送信待ち';
    case 'queued':
      return '準備中（GPU を起動しています）';
    case 'downloading':
      return '動画を読み込んでいます';
    case 'tracking':
      return `処理中 ${Math.floor((progress ?? 0) * 100)}%`;
    case 'finishing':
      return '仕上げています';
    case 'done':
      return '完了しました';
    case 'failed':
      return '失敗しました';
    default:
      return '処理中';
  }
}

/** API のエラーコード（{code}）→ 文言 */
const API_ERROR_MESSAGES = {
  QUOTA_EXCEEDED: '今日の回数を使い切りました（1日3回まで）。明日また試してください',
  VIDEO_CUTOUT_DISABLED: '動画の切り抜きは現在停止中です。時間をおいて試してください',
  TOO_LARGE: '動画が大きすぎます（200MB まで）',
  UNSUPPORTED_CONTENT_TYPE: 'この形式の動画には対応していません（MP4 / MOV のみ）',
  INVALID_RANGE: '範囲が正しくありません（10 秒まで）。選び直してください',
  INVALID_POINTS: '点の指定が正しくありません。点を打ち直してください',
  UPLOAD_NOT_FOUND: '動画のアップロードが確認できませんでした。アップロードをやり直してください',
  VIDEO_JOB_INVALID_STATE: 'このジョブはすでに送信済みです',
  VIDEO_JOB_NOT_FOUND: SERVER_RESTARTED_MESSAGE,
};

/** api.js が投げたエラー（err.code / err.message）を画面に出す文言にする */
export function apiErrorMessage(err) {
  return (err && API_ERROR_MESSAGES[err.code]) || (err && err.message) || 'エラーが発生しました';
}

/** 今日の残り回数の表示。まだ分からない（ジョブを作る前）なら上限だけ出す */
export function remainingMessage(remainingToday) {
  if (remainingToday === null || remainingToday === undefined) return '1日3回まで';
  return `今日の残り ${remainingToday} 回（1日3回まで）`;
}
