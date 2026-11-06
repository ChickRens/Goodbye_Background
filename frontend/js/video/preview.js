/**
 * 開始位置のフレームで「どこを切り抜くか」を決めるプレビュー。
 *
 * 既存の画像用 API をそのまま使う: フレームの PNG を /upload → タップごとに /segment、
 * 取り消しは /undo・/redo、表示は /get-preview。canvas の描画とタップの入力は既存の canvas.js。
 * 既存の画像編集（editor.js）と同じく、変更系のリクエストは同時に 1 つだけにする（busy）。
 *
 * 座標: canvas の解像度は /get-preview の画像（プレビュー画像）の実ピクセルなので、
 * タップ点はプレビュー画像の座標で届く。動画ジョブには toNormalized で 0〜1 にして送る
 * （仕様 §3。既存の 3 つの座標系とは混ざらない）。
 *
 * 点の一覧はサーバーが返さないので、editor.js と同じく
 * 「undo 1 回 = 最後の 1 点を外す / redo 1 回 = 外した 1 点を戻す」でこちらでも持つ。
 * 送信するのは undo/redo を反映した今の点の一覧。
 */
import { uploadImage, getPreviewImage, segmentImage, undoEdit, redoEdit } from '../api.js';
import { createCanvasController } from '../canvas.js';

/** 動画ジョブに送れる点の上限（サーバーの MAX_POINTS と同じ値） */
export const MAX_POINTS = 8;

/** プレビュー画像の座標 → 0〜1 の正規化座標（範囲外は 0〜1 に丸め込む） */
export function toNormalized(previewX, previewY, previewW, previewH) {
  return {
    x: clamp01(previewX / previewW),
    y: clamp01(previewY / previewH),
  };
}

function clamp01(value) {
  if (!Number.isFinite(value)) return 0;
  return Math.min(1, Math.max(0, value));
}

/**
 * @param {Object} opts
 * @param {HTMLCanvasElement} opts.canvasEl
 * @param {HTMLButtonElement} opts.modeToggleBtn
 * @param {HTMLButtonElement} opts.undoBtn
 * @param {HTMLButtonElement} opts.redoBtn
 * @param {HTMLElement} opts.statusEl
 * @param {() => void} opts.onChange 点の数・busy・読み込み状態が変わったら呼ぶ（送信ボタンの更新用）
 * @param {(err: Error, what: string) => void} opts.onError
 */
export function createFramePreview({ canvasEl, modeToggleBtn, undoBtn, redoBtn, statusEl, onChange, onError }) {
  const controller = createCanvasController(canvasEl, { onPointAdd: handlePointAdd });

  let generation = 0; // reset のたびに進める。古いリクエストの応答を捨てるため
  let sessionId = null;
  let size = null; // プレビュー画像の実ピクセル { width, height }
  let points = []; // { x, y, label: 1 | 0 }（プレビュー画像の座標）
  let redoStack = [];
  let busy = false;
  let locked = false; // 送信後は点を変えさせない
  let previewUrl = null;
  let frameUrl = null;

  modeToggleBtn.addEventListener('click', () => {
    applyMode(modeToggleBtn.dataset.mode === 'positive' ? 'negative' : 'positive');
  });
  undoBtn.addEventListener('click', () => runHistory(undoEdit, popLastPoint));
  redoBtn.addEventListener('click', () => runHistory(redoEdit, restoreLastPoint));
  applyMode('positive');
  sync();

  function applyMode(mode) {
    modeToggleBtn.dataset.mode = mode;
    modeToggleBtn.textContent = mode === 'positive' ? '➕ 含める' : '➖ 除外する';
    controller.setPointMode(mode);
  }

  function sync() {
    const disabled = busy || locked || !sessionId;
    undoBtn.disabled = disabled || points.length === 0;
    redoBtn.disabled = disabled || redoStack.length === 0;
    modeToggleBtn.disabled = locked || !sessionId;
    onChange();
  }

  function setStatus(text) {
    statusEl.textContent = text;
  }

  function setBusy(value) {
    busy = value;
    sync();
  }

  function replacePreviewUrl(url) {
    if (previewUrl && previewUrl !== url) URL.revokeObjectURL(previewUrl);
    previewUrl = url;
  }

  /**
   * フレームの PNG を既存の /upload に送り、最初のプレビューを出す。
   * 失敗したら false（同じ Blob でもう一度呼べばやり直せる）。
   */
  async function load(frameBlob) {
    const gen = generation;
    setBusy(true);
    setStatus('プレビューを準備中…');
    if (frameUrl) URL.revokeObjectURL(frameUrl);
    frameUrl = URL.createObjectURL(frameBlob);
    let url = null;
    try {
      const file = new File([frameBlob], 'frame.png', { type: 'image/png' });
      const uploaded = await uploadImage(file);
      if (gen !== generation) return;
      url = await getPreviewImage(uploaded.imageId);
      if (gen !== generation) return;
      const loaded = await controller.loadBaseImage(url, () => gen !== generation);
      if (!loaded) return;
      replacePreviewUrl(url);
      url = null;
      sessionId = uploaded.sessionId;
      size = loaded;
      controller.loadOriginalImage(frameUrl, () => gen !== generation);
      setStatus('残したい所をタップしてください');
      return true;
    } catch (err) {
      if (gen === generation) {
        setStatus('');
        onError(err, 'プレビューの準備');
      }
      return false;
    } finally {
      if (url) URL.revokeObjectURL(url);
      if (gen === generation) setBusy(false);
    }
  }

  function handlePointAdd(point) {
    if (!sessionId || locked) return;
    if (busy) {
      setStatus('処理中です。完了までお待ちください');
      return;
    }
    if (points.length >= MAX_POINTS) {
      setStatus(`点は ${MAX_POINTS} 個までです。Undo で減らしてから打ち直してください`);
      return;
    }
    const previousRedoStack = redoStack;
    points = [...points, point];
    redoStack = [];
    controller.setPoints(points);
    runSegment(point, previousRedoStack);
  }

  async function runSegment(point, previousRedoStack) {
    const gen = generation;
    setBusy(true);
    setStatus('切り抜きを計算中…');
    let applied = false;
    try {
      const { imageId } = await segmentImage(sessionId, point);
      applied = true; // ここから先の失敗はサーバー側では適用済み。点は戻さない
      if (gen !== generation) return;
      await showPreview(imageId, gen);
      setStatus('');
    } catch (err) {
      if (gen !== generation) return;
      if (!applied) {
        points = points.filter((p) => p !== point);
        redoStack = previousRedoStack;
        controller.setPoints(points);
      }
      setStatus('');
      onError(err, '切り抜きの計算');
    } finally {
      if (gen === generation) setBusy(false);
    }
  }

  function popLastPoint() {
    if (points.length === 0) return;
    redoStack = [...redoStack, points[points.length - 1]];
    points = points.slice(0, -1);
    controller.setPoints(points);
  }

  function restoreLastPoint() {
    if (redoStack.length === 0) return;
    points = [...points, redoStack[redoStack.length - 1]];
    redoStack = redoStack.slice(0, -1);
    controller.setPoints(points);
  }

  async function runHistory(apiFn, localPointSync) {
    if (!sessionId || busy || locked) return;
    const gen = generation;
    setBusy(true);
    setStatus('処理中…');
    try {
      const { imageId } = await apiFn(sessionId);
      if (gen !== generation) return;
      localPointSync(); // サーバーの履歴は進んだので、点の一覧も合わせる
      await showPreview(imageId, gen);
      setStatus('');
    } catch (err) {
      if (gen !== generation) return;
      setStatus('');
      onError(err, '取り消し・やり直し');
    } finally {
      if (gen === generation) setBusy(false);
    }
  }

  async function showPreview(imageId, gen) {
    const url = await getPreviewImage(imageId);
    if (gen !== generation) {
      URL.revokeObjectURL(url);
      return;
    }
    replacePreviewUrl(url);
    await controller.loadBaseImage(url, () => gen !== generation);
  }

  /** 動画ジョブに送る点（undo/redo を反映した今の一覧、0〜1 の正規化座標） */
  function videoPoints() {
    if (!size) return [];
    return points.map((p) => ({
      ...toNormalized(p.x, p.y, size.width, size.height),
      label: p.label === 1 ? 'include' : 'exclude',
    }));
  }

  /** 送信後は点を変えさせない */
  function lock() {
    locked = true;
    setStatus('');
    sync();
  }

  /** 送信に失敗して、点を直してもう一度送れるようにするとき */
  function unlock() {
    locked = false;
    sync();
  }

  /** 最初からやり直すとき。進行中の応答は generation で捨てる */
  function reset() {
    generation += 1;
    sessionId = null;
    size = null;
    points = [];
    redoStack = [];
    busy = false;
    locked = false;
    replacePreviewUrl(null);
    if (frameUrl) URL.revokeObjectURL(frameUrl);
    frameUrl = null;
    controller.reset();
    applyMode('positive');
    setStatus('');
    sync();
  }

  return {
    load,
    lock,
    unlock,
    reset,
    videoPoints,
    pointCount: () => points.length,
    isReady: () => sessionId !== null && !busy,
  };
}
