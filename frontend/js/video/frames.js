/**
 * 点を打つフレーム（複数フレームの点の指定）の判定だけをまとめた純粋な関数。画面は page.js、1 フレーム分の点とプレビューは preview.js。
 * 仕様: docs/superpowers/specs/2026-11-09-video-multi-frame-prompts-design.md
 */

/** 点を打てるフレームの数（開始位置を含む。サーバーの MAX_PROMPTS と同じ） */
export const MAX_FRAMES = 5;

/**
 * そのフレームを追加してよいか。
 * @param {number[]} times 追加済みのフレームの時刻（元の動画のミリ秒）
 * @param {number} timeMs 追加したい時刻
 * @param {number} startMs 範囲の開始
 * @param {number} endMs 範囲の終わり（この時刻は含まない）
 */
export function canAddFrame(times, timeMs, startMs, endMs) {
  const t = Math.round(timeMs);
  return times.length < MAX_FRAMES && startMs <= t && t < endMs && !times.some((x) => Math.round(x) === t);
}

/**
 * 送信する prompts（時刻の昇順。1 つ目は開始位置のフレーム）。
 * @param {{ timeMs: number, points: Array<{x: number, y: number, label: string}> }[]} frames
 */
export function buildPrompts(frames) {
  return [...frames]
    .sort((a, b) => a.timeMs - b.timeMs)
    .map((f) => ({ time_ms: Math.round(f.timeMs), points: f.points }));
}

/** 全部のフレームで点が打たれ、プレビューで対象が見つかっている（= 送信できる） */
export function allFramesReady(frames) {
  return frames.length > 0 && frames.every((f) => f.canSend);
}

/** タブに出す時刻（範囲の開始からの経過。m:ss.s） */
export function formatFrameTime(timeMs, startMs) {
  const tenths = Math.round((timeMs - startMs) / 100);
  const minutes = Math.floor(tenths / 600);
  const seconds = (tenths % 600) / 10;
  return `${minutes}:${seconds.toFixed(1).padStart(4, '0')}`;
}
