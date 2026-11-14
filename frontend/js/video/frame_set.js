/**
 * 点を打つフレームの組（複数フレームの点の指定）。1 フレーム分の点とプレビューは preview.js の createFramePreview に任せ、
 * ここはフレームごとのパネル（キャンバス・状態・警告）とタブを作り、表示中のフレームを切り替え、送信の可否をまとめる。
 *
 * - 1 枚目は範囲の開始位置（削除できない）。2 枚目以降は利用者が選んだ時刻で、最大 MAX_FRAMES 枚
 * - モード切り替え・Undo・Redo のボタンは共有。表示中のフレームの preview だけが反応する（setActive）
 * 仕様: docs/superpowers/specs/2026-11-09-video-multi-frame-prompts-design.md §6
 */

import { createFramePreview } from './preview.js';
import { MAX_FRAMES, allFramesReady, buildPrompts, canAddFrame, formatFrameTime } from './frames.js';
import { toJobTime } from './trim.js';

/**
 * @param {Object} opts
 * @param {HTMLElement} opts.tabsEl タブを並べる所
 * @param {HTMLElement} opts.panelsEl パネルを入れる所
 * @param {HTMLTemplateElement} opts.panelTemplate canvas・.edit-status・.video-alert を持つパネルのひな形
 * @param {HTMLButtonElement} opts.modeToggleBtn
 * @param {HTMLButtonElement} opts.undoBtn
 * @param {HTMLButtonElement} opts.redoBtn
 * @param {() => void} opts.onChange
 * @param {(err: Error, what: string) => void} opts.onError
 */
export function createFrameSet({ tabsEl, panelsEl, panelTemplate, modeToggleBtn, undoBtn, redoBtn, onChange, onError }) {
  /** @type {{ timeMs: number, preview: any, panel: HTMLElement, tab: HTMLElement, frameBlob: Blob }[]} */
  let frames = [];
  let active = null;
  let startMs = 0;
  let endMs = 0;
  let offsetMs = 0; // アップロードした動画の時刻 0 = 元の動画の offsetMs（trim.js）
  let jobId = null;
  let locked = false;

  function makeFrame(timeMs, frameBlob) {
    const panel = panelTemplate.content.firstElementChild.cloneNode(true);
    panel.hidden = true;
    panelsEl.appendChild(panel);
    const frame = { timeMs, frameBlob, panel, tab: null, preview: null };
    frame.preview = createFramePreview({
      canvasEl: panel.querySelector('canvas'),
      modeToggleBtn,
      undoBtn,
      redoBtn,
      statusEl: panel.querySelector('.edit-status'),
      warningEl: panel.querySelector('.video-alert'),
      onChange: () => {
        renderTabs();
        onChange();
      },
      onError,
    });
    frame.preview.setActive(false);
    frames = [...frames, frame].sort((a, b) => a.timeMs - b.timeMs);
    return frame;
  }

  function select(frame) {
    active = frame;
    for (const f of frames) {
      const on = f === frame;
      f.panel.hidden = !on;
      f.preview.setActive(on);
    }
    renderTabs();
    onChange();
  }

  /** フレームの準備の状態をタブの印にする（✓ = 送れる、! = 対象なし、無印 = まだ） */
  function mark(f) {
    if (f.preview.canSend()) return ' ✓';
    if (f.preview.objectMissing()) return ' !';
    return '';
  }

  function renderTabs() {
    tabsEl.replaceChildren(
      ...frames.map((f, i) => {
        const tab = document.createElement('div');
        tab.className = 'frame-tab';
        const btn = document.createElement('button');
        btn.type = 'button';
        btn.className = 'frame-tab-select';
        btn.setAttribute('role', 'tab');
        btn.setAttribute('aria-selected', String(f === active));
        btn.textContent = `${formatFrameTime(f.timeMs, startMs)}${mark(f)}`;
        btn.addEventListener('click', () => select(f));
        tab.appendChild(btn);
        if (i > 0 && !locked) {
          const del = document.createElement('button');
          del.type = 'button';
          del.className = 'frame-tab-delete';
          del.setAttribute('aria-label', `${formatFrameTime(f.timeMs, startMs)} のフレームを消す`);
          del.textContent = '×';
          del.addEventListener('click', () => remove(f));
          tab.appendChild(del);
        }
        f.tab = tab;
        return tab;
      }),
    );
  }

  function remove(frame) {
    if (locked || frame === frames[0]) return;
    // 消したフレームの preview も共有のボタンにリスナーを持ったままなので、必ず受け持ちを外す
    // （外さないと、モードのボタンの表示を自分のモードで上書きし、表示と実際のモードがずれる）
    frame.preview.setActive(false);
    frame.preview.reset();
    frame.panel.remove();
    frames = frames.filter((f) => f !== frame);
    if (active === frame) select(frames[0]);
    else {
      renderTabs();
      onChange();
    }
  }

  /** サーバーに送る時刻（アップロードした動画を切り出したときは、その分ずらす。trim.js） */
  function jobTime(frame) {
    return toJobTime(frame.timeMs, offsetMs);
  }

  /**
   * 範囲の開始位置のフレームで始める。失敗したら false（retry でやり直せる）。
   * 時刻は元の動画のミリ秒。newOffsetMs はアップロードした動画の時刻 0 が元の動画の何ミリ秒か（切り出していなければ 0）
   */
  async function start(newJobId, newStartMs, newEndMs, frameBlob, newOffsetMs = 0) {
    jobId = newJobId;
    startMs = newStartMs;
    endMs = newEndMs;
    offsetMs = newOffsetMs;
    const first = frames[0] ?? makeFrame(newStartMs, frameBlob);
    select(first);
    return first.preview.load(jobId, first.frameBlob, jobTime(first));
  }

  /** そのフレームを追加して表示し、登録する。追加できない時刻なら null、登録に失敗したら false */
  async function add(timeMs, frameBlob) {
    const t = Math.round(timeMs);
    if (locked || !jobId || !canAddFrame(times(), t, startMs, endMs)) return null;
    const frame = makeFrame(t, frameBlob);
    select(frame);
    return frame.preview.load(jobId, frame.frameBlob, jobTime(frame));
  }

  /** 表示中のフレームの登録をやり直す（「プレビューをやり直す」） */
  function retryActive() {
    if (!active || !jobId) return Promise.resolve(false);
    return active.preview.load(jobId, active.frameBlob, jobTime(active));
  }

  function times() {
    return frames.map((f) => f.timeMs);
  }

  /** 送信の可否の説明に使う: 最初に見つかった「まだ送れない」フレーム */
  function firstProblem() {
    for (const f of frames) {
      const label = formatFrameTime(f.timeMs, startMs);
      if (f.preview.pointCount() === 0) return { kind: 'noPoints', label };
      if (f.preview.objectMissing()) return { kind: 'missing', label };
      if (!f.preview.canSend()) return { kind: 'pending', label };
    }
    return null;
  }

  function lock() {
    locked = true;
    for (const f of frames) f.preview.lock();
    renderTabs();
  }

  function unlock() {
    locked = false;
    for (const f of frames) f.preview.unlock();
    renderTabs();
  }

  /** 最初からやり直すとき: 全部のフレームを捨てる */
  function reset() {
    for (const f of frames) {
      f.preview.setActive(false); // remove() と同じ理由
      f.preview.reset();
      f.panel.remove();
    }
    frames = [];
    active = null;
    jobId = null;
    locked = false;
    tabsEl.replaceChildren();
    onChange();
  }

  return {
    start,
    add,
    retryActive,
    times,
    canAdd: (timeMs) => !locked && jobId !== null && canAddFrame(times(), timeMs, startMs, endMs),
    isFull: () => frames.length >= MAX_FRAMES,
    hasFrames: () => frames.length > 0,
    canSend: () => allFramesReady(frames.map((f) => ({ canSend: f.preview.canSend() }))),
    activeCanSendOrMissing: () => !!active && (active.preview.canSend() || active.preview.objectMissing()),
    firstProblem,
    prompts: () => buildPrompts(frames.map((f) => ({ timeMs: jobTime(f), points: f.preview.videoPoints() }))),
    lock,
    unlock,
    reset,
  };
}
