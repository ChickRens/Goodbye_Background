/**
 * 動画の範囲（開始・終了ミリ秒）の丸め込みと、開始位置のフレームの切り出し。
 *
 * 範囲の約束（サーバーの ClipRange と同じ）: 0 ≤ 開始 < 終了 ≤ 動画の長さ、長さは maxMs（10 秒）まで。
 * clampRange は「開始 < 終了」までは保証しない（開始 = 終了になり得る）。
 * 送る前に page.js 側で「終了 > 開始」を確かめる。
 */

/** 切り出せる長さの上限（ms）。サーバーの MAX_CLIP_MS と同じ値 */
export const MAX_CLIP_MS = 10_000;

/**
 * 開始・終了を動画の長さと上限の長さに収める。
 * 開始は 0〜動画の長さ、終了は 開始〜min(開始 + maxMs, 動画の長さ) に丸め込む。
 * 返す値は整数ミリ秒（サーバーは u32 で受ける）。
 */
export function clampRange(startMs, endMs, durationMs, maxMs = MAX_CLIP_MS) {
  const duration = Math.max(0, Math.floor(durationMs));
  const start = clamp(Math.round(startMs), 0, duration);
  const end = clamp(Math.round(endMs), start, Math.min(start + maxMs, duration));
  return { startMs: start, endMs: end };
}

function clamp(value, min, max) {
  return Math.min(Math.max(value, min), max);
}

/**
 * プレビュー用に切り出すフレームの長辺の上限。
 * 既存の /upload は 10MB まで。4K のフレームを PNG にすると超え得るので縮小する
 * （タップ点は 0〜1 の正規化座標で送るので、縮小しても座標はずれない）。
 */
const FRAME_MAX_LONG_SIDE = 2000;
const SEEK_TIMEOUT_MS = 10_000;

/**
 * video を ms の位置へ移動し、`seeked` を待ってから canvas に描いて PNG にする。
 * ブラウザが表示する「その時刻のフレーム」を使う（ワーカーの ffmpeg とは ±1 フレームずれ得るが、
 * トラッキングが吸収する。仕様 §3）。
 */
export async function captureFrame(video, ms) {
  await seekTo(video, ms / 1000);

  const width = video.videoWidth;
  const height = video.videoHeight;
  if (!width || !height) {
    throw new Error('動画のフレームを読み込めませんでした');
  }
  const scale = Math.min(1, FRAME_MAX_LONG_SIDE / Math.max(width, height));
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, Math.round(width * scale));
  canvas.height = Math.max(1, Math.round(height * scale));
  canvas.getContext('2d').drawImage(video, 0, 0, canvas.width, canvas.height);

  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (blob) resolve(blob);
      else reject(new Error('フレームを画像にできませんでした'));
    }, 'image/png');
  });
}

/** currentTime を変えて `seeked` を待つ。同じ位置にいて描画できる状態なら待たない（seeked が来ないため） */
function seekTo(video, seconds) {
  if (Math.abs(video.currentTime - seconds) < 0.001 && video.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA) {
    return Promise.resolve();
  }
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      cleanup();
      reject(new Error('動画の指定位置に移動できませんでした'));
    }, SEEK_TIMEOUT_MS);
    const onSeeked = () => {
      cleanup();
      resolve();
    };
    const onError = () => {
      cleanup();
      reject(new Error('動画を読み込めませんでした'));
    };
    function cleanup() {
      clearTimeout(timer);
      video.removeEventListener('seeked', onSeeked);
      video.removeEventListener('error', onError);
    }
    video.addEventListener('seeked', onSeeked);
    video.addEventListener('error', onError);
    video.currentTime = seconds;
  });
}

/** 表示用: 12345 → "0:12.3" */
export function formatMs(ms) {
  const totalTenths = Math.floor(ms / 100);
  const minutes = Math.floor(totalTenths / 600);
  const seconds = Math.floor((totalTenths % 600) / 10);
  const tenths = totalTenths % 10;
  return `${minutes}:${String(seconds).padStart(2, '0')}.${tenths}`;
}
