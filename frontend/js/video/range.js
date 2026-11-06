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
 * video を ms の位置へ移動し、そのフレームが描ける状態になるのを待ってから canvas に描いて PNG にする。
 * ブラウザが表示する「その時刻のフレーム」を使う（ワーカーの ffmpeg とは ±1 フレームずれ得るが、
 * トラッキングが吸収する。仕様 §3）。
 *
 * iOS Safari では `seeked` がフレームのデコードより先に来て、真っ黒（透明）な canvas になることがある。そのため:
 *   1. 再生を止めてから移動する
 *   2. `seeked` の後、readyState が HAVE_CURRENT_DATA 未満なら `loadeddata` / `canplay` を待つ
 *   3. requestVideoFrameCallback があれば、移動先のフレームが表示されるのも待つ（来なければ少し待って進む）
 *   4. 描いた結果を小さく縮めて調べ、全部が黒か透明なら、ミュートで play() → pause() してデコードを促し 1 回だけやり直す
 * それでも描けなければ例外にする（画面の「プレビューをやり直す」で再挑戦できる）。
 */
export async function captureFrame(video, ms) {
  const seconds = ms / 1000;
  video.pause();
  await seekTo(video, seconds);

  let canvas = drawFrame(video);
  if (isBlank(canvas)) {
    await nudgeDecode(video, seconds);
    canvas = drawFrame(video);
    if (isBlank(canvas)) {
      throw new Error('開始位置のフレームを描けませんでした。「プレビューをやり直す」を試すか、最初からやり直して開始位置を少しずらしてください');
    }
  }

  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (blob) resolve(blob);
      else reject(new Error('フレームを画像にできませんでした'));
    }, 'image/png');
  });
}

function drawFrame(video) {
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
  return canvas;
}

const BLANK_SAMPLE_SIZE = 16;
const BLANK_MAX_CHANNEL = 8; // これ以下の明るさしか無ければ「真っ黒」とみなす

/** 描いた canvas を 16×16 に縮めて、全部が透明か真っ黒かを調べる */
function isBlank(canvas) {
  const sample = document.createElement('canvas');
  sample.width = BLANK_SAMPLE_SIZE;
  sample.height = BLANK_SAMPLE_SIZE;
  const ctx = sample.getContext('2d', { willReadFrequently: true });
  ctx.drawImage(canvas, 0, 0, BLANK_SAMPLE_SIZE, BLANK_SAMPLE_SIZE);
  const { data } = ctx.getImageData(0, 0, BLANK_SAMPLE_SIZE, BLANK_SAMPLE_SIZE);
  for (let i = 0; i < data.length; i += 4) {
    const alpha = data[i + 3];
    if (alpha !== 0 && Math.max(data[i], data[i + 1], data[i + 2]) > BLANK_MAX_CHANNEL) return false;
  }
  return true;
}

/** デコードを促す: ミュートで少しだけ再生して止め、指定位置へ戻る */
async function nudgeDecode(video, seconds) {
  video.muted = true;
  try {
    await video.play();
  } catch {
    /* 自動再生が拒まれても、移動し直すだけで描けることがあるので続ける */
  }
  video.pause();
  await seekTo(video, seconds, { force: true });
}

const FRAME_CALLBACK_GRACE_MS = 500;

/**
 * currentTime を変えて、フレームが描ける状態になるまで待つ（全体で SEEK_TIMEOUT_MS まで）。
 * 同じ位置にいて描ける状態なら移動しない（`seeked` が来ないため）。force なら必ず移動し直す。
 */
function seekTo(video, seconds, { force = false } = {}) {
  const alreadyThere = Math.abs(video.currentTime - seconds) < 0.001;
  if (alreadyThere && !force && video.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA) {
    return Promise.resolve();
  }
  return new Promise((resolve, reject) => {
    let finished = false;
    let seeked = false;
    let framePresented = typeof video.requestVideoFrameCallback !== 'function';
    let graceTimer = null;

    const timer = setTimeout(() => finish(new Error('動画の指定位置に移動できませんでした')), SEEK_TIMEOUT_MS);

    function finish(err) {
      if (finished) return;
      finished = true;
      clearTimeout(timer);
      clearTimeout(graceTimer);
      video.removeEventListener('seeked', onSeeked);
      video.removeEventListener('loadeddata', check);
      video.removeEventListener('canplay', check);
      video.removeEventListener('error', onError);
      if (err) reject(err);
      else resolve();
    }

    function check() {
      if (seeked && framePresented && video.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA) finish();
    }

    function onSeeked() {
      seeked = true;
      // 止まっている動画では requestVideoFrameCallback が来ないブラウザもあるので、少し待ったら諦めて進む
      if (!framePresented) {
        graceTimer = setTimeout(() => {
          framePresented = true;
          check();
        }, FRAME_CALLBACK_GRACE_MS);
      }
      check();
    }

    function onError() {
      finish(new Error('動画を読み込めませんでした'));
    }

    video.addEventListener('seeked', onSeeked);
    video.addEventListener('loadeddata', check);
    video.addEventListener('canplay', check);
    video.addEventListener('error', onError);
    if (!framePresented) {
      video.requestVideoFrameCallback(() => {
        framePresented = true;
        check();
      });
    }
    // 同じ位置への代入でも seeking/seeked は起きる（force のとき）
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
