/**
 * 開始位置のフレームで「どこを切り抜くか」を決めるプレビュー。
 *
 * 動画ジョブ専用の API を使う（本番のワーカーと同じ SAM2.1 tiny の 1 フレーム目をサーバーの CPU で計算する。仕様 D8）:
 *   1. フレームを POST /video-jobs/{id}/preview-frame で 1 回だけ登録する（サーバーがエンコードして持つ）
 *   2. 点が変わるたびに、今の点の一覧を丸ごと POST /video-jobs/{id}/preview に送り、alpha マスク（PNG）を受け取る
 *   3. フレームにマスクを alpha として合成し、canvas.js で市松模様の上に表示する（消える所は薄いフレームが見える）
 * どちらも回数は消費しない。
 *
 * 点の一覧はこちらで持つ。Undo / Redo は手元の一覧を戻す・進めるだけで、そのたびに一覧を送り直す。
 * リクエストは同時に 1 つだけ。応答待ちの間に点が変わったら、応答を捨てて最新の一覧でもう一度送る
 * （画面に出すのは「今の一覧」に対する応答だけ）。
 *
 * サーバーの事情によるやり直し:
 *   - 503 PREVIEW_BUSY（フレームの登録はサーバー全体で同時に 2 つまで）: 1 秒・2 秒・4 秒待って登録し直す
 *   - 409 PREVIEW_FRAME_NOT_FOUND（混雑時や期限切れでサーバーが特徴量を捨てた）: 同じフレームを登録し直して 1 回だけやり直す
 *
 * 座標: canvas の解像度はフレーム画像の実ピクセルなので、タップ点はフレームの座標で届く。
 * 動画ジョブには toNormalized で 0〜1 にして送る（仕様 §3。既存の 3 つの座標系とは混ざらない）。
 */
import { setVideoPreviewFrame, previewVideoMask } from '../api.js';
import { createCanvasController } from '../canvas.js';

/** 動画ジョブに送れる点の上限（サーバーの MAX_POINTS と同じ値） */
export const MAX_POINTS = 8;

/** PREVIEW_BUSY のときに待つ時間（ms）。この回数だけやり直す */
export const BUSY_RETRY_DELAYS_MS = [1000, 2000, 4000];

/** /preview-frame が受け付けるフレームの大きさ（サーバーと同じ 10MB）。PNG で超えたら JPEG にする */
const MAX_FRAME_BYTES = 10 * 1024 * 1024;
const FALLBACK_JPEG_QUALITY = 0.92;

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
 * フレームの RGBA にマスク（グレースケール。R = G = B = 濃さ）の濃さを alpha として入れる。
 * frame は書き換える。2 つは同じ長さ（同じ大きさの画像）であること。
 */
export function applyMaskAlpha(frame, mask) {
  for (let i = 3; i < frame.length; i += 4) {
    frame[i] = mask[i - 3];
  }
  return frame;
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function loadImage(url) {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error('画像の読み込みに失敗しました'));
    img.src = url;
  });
}

async function loadImageFromBlob(blob) {
  const url = URL.createObjectURL(blob);
  try {
    return await loadImage(url);
  } finally {
    URL.revokeObjectURL(url);
  }
}

/**
 * フレームにマスクを alpha として合成し、canvas に描く（マスクの大きさが違えばフレームの大きさに引き伸ばす）。
 * canvas は使い回す（iOS Safari は canvas のメモリの合計に上限があり、タップのたびに最大 2000px の canvas を
 * 作ると超え得るため）。大きさはフレームが変わったときだけ変える。表示中の canvas に描き直しても、
 * この関数は同期で終わるので、途中の状態が描画（requestAnimationFrame）に出ることは無い。
 */
function composeCutout(canvas, frameImg, maskImg) {
  const width = frameImg.naturalWidth;
  const height = frameImg.naturalHeight;
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
  }
  const ctx = canvas.getContext('2d', { willReadFrequently: true });
  ctx.clearRect(0, 0, width, height);
  ctx.drawImage(maskImg, 0, 0, width, height);
  const mask = ctx.getImageData(0, 0, width, height).data;
  ctx.clearRect(0, 0, width, height);
  ctx.drawImage(frameImg, 0, 0, width, height);
  const frame = ctx.getImageData(0, 0, width, height);
  applyMaskAlpha(frame.data, mask);
  ctx.putImageData(frame, 0, 0);
  return canvas;
}

/** 10MB を超える PNG は JPEG にする（サーバーは PNG / JPEG を受け付ける） */
function frameForUpload(frameBlob, frameImg) {
  if (frameBlob.size <= MAX_FRAME_BYTES) return Promise.resolve(frameBlob);
  const canvas = document.createElement('canvas');
  canvas.width = frameImg.naturalWidth;
  canvas.height = frameImg.naturalHeight;
  canvas.getContext('2d').drawImage(frameImg, 0, 0);
  return new Promise((resolve, reject) => {
    canvas.toBlob(
      (blob) => (blob ? resolve(blob) : reject(new Error('フレームを画像にできませんでした'))),
      'image/jpeg',
      FALLBACK_JPEG_QUALITY,
    );
  });
}

/**
 * @param {Object} opts
 * @param {HTMLCanvasElement} opts.canvasEl
 * @param {HTMLButtonElement} opts.modeToggleBtn
 * @param {HTMLButtonElement} opts.undoBtn
 * @param {HTMLButtonElement} opts.redoBtn
 * @param {HTMLElement} opts.statusEl
 * @param {HTMLElement} opts.warningEl 対象が見つからないときに出す欄
 * @param {() => void} opts.onChange 点の数・計算中・結果が変わったら呼ぶ（送信ボタンの更新用）
 * @param {(err: Error, what: string) => void} opts.onError
 *
 * 複数フレームの点の指定（frame_set.js）では、フレームごとに 1 つ作り、モード切り替え・Undo・Redo のボタンは共有する。
 * 共有のボタンに反応し、その表示を更新するのは setActive(true) のもの（表示中のフレーム）だけ。
 */
export function createFramePreview({ canvasEl, modeToggleBtn, undoBtn, redoBtn, statusEl, warningEl, onChange, onError }) {
  const controller = createCanvasController(canvasEl, { onPointAdd: handlePointAdd });

  let generation = 0; // reset のたびに進める。前の動画の応答を捨てるため
  let jobId = null;
  let timeMs = 0; // このフレームの時刻（サーバーに送るジョブの時刻。アップロードした動画を切り出したときはずらした値。frame_set.js）
  let frameBlob = null;
  let frameUrl = null;
  let frameImg = null;
  let uploadBlob = null; // 登録に送るフレーム（10MB を超える PNG は JPEG にしたもの）。登録し直しで作り直さない
  let registered = false; // サーバーにフレームを登録できた
  const cutoutCanvas = document.createElement('canvas'); // 合成した切り抜き（使い回す）
  let points = []; // { x, y, label: 1 | 0 }（フレームの座標）
  let redoStack = [];
  let version = 0; // 点の一覧が変わるたびに進める
  let shown = { version: 0, objectPresent: null }; // 画面に出ているマスクがどの一覧のものか
  let computing = false; // /preview の応答待ち（登録し直しを含む）
  let locked = false; // 送信後は点を変えさせない
  let active = true; // 共有のボタン（モード・Undo・Redo）を受け持つか
  let mode = 'positive'; // このフレームの「含める／除外する」

  modeToggleBtn.addEventListener('click', () => {
    if (!active) return;
    applyMode(mode === 'positive' ? 'negative' : 'positive');
  });
  undoBtn.addEventListener('click', () => {
    if (!active || locked || points.length === 0) return;
    redoStack = [...redoStack, points[points.length - 1]];
    points = points.slice(0, -1);
    pointsChanged();
  });
  redoBtn.addEventListener('click', () => {
    if (!active || locked || redoStack.length === 0) return;
    points = [...points, redoStack[redoStack.length - 1]];
    redoStack = redoStack.slice(0, -1);
    pointsChanged();
  });
  applyMode('positive');
  sync();

  function applyMode(newMode) {
    mode = newMode;
    controller.setPointMode(mode);
    if (!active) return;
    modeToggleBtn.dataset.mode = mode;
    modeToggleBtn.textContent = mode === 'positive' ? '➕ 含める' : '➖ 除外する';
  }

  /** 表示中のフレームにする（true）／外す（false）。表示中のものだけが共有のボタンを受け持つ */
  function setActive(value) {
    active = value;
    if (active) applyMode(mode);
    sync();
  }

  function isCurrent() {
    return shown.version === version;
  }

  function sync() {
    if (active) {
      const disabled = locked || !registered;
      undoBtn.disabled = disabled || points.length === 0;
      redoBtn.disabled = disabled || redoStack.length === 0;
      modeToggleBtn.disabled = disabled;
    }
    warningEl.hidden = !(isCurrent() && shown.objectPresent === false);
    onChange();
  }

  function setStatus(text) {
    statusEl.textContent = text;
  }

  /** PREVIEW_BUSY なら待ってやり直す。待つ間は「混み合っています…」を出す */
  async function withBusyRetry(fn, gen) {
    for (let attempt = 0; ; attempt += 1) {
      try {
        return await fn();
      } catch (err) {
        if (err.code !== 'PREVIEW_BUSY' || attempt >= BUSY_RETRY_DELAYS_MS.length || gen !== generation) throw err;
        setStatus('混み合っています…');
        await sleep(BUSY_RETRY_DELAYS_MS[attempt]);
        if (gen !== generation) throw err;
      }
    }
  }

  async function registerFrame(gen) {
    if (!uploadBlob) uploadBlob = await frameForUpload(frameBlob, frameImg);
    const blob = uploadBlob;
    await withBusyRetry(() => setVideoPreviewFrame(jobId, blob, timeMs), gen);
  }

  /**
   * ジョブにフレームを登録し、タップできる状態にする。点があれば（やり直しのとき）計算し直す。
   * 失敗したら false（同じ引数でもう一度呼べばやり直せる）。
   */
  async function load(newJobId, newFrameBlob, newTimeMs) {
    const gen = generation;
    registered = false;
    sync();
    setStatus('プレビューを準備中…');
    try {
      if (frameBlob !== newFrameBlob || !frameImg) {
        jobId = newJobId;
        frameBlob = newFrameBlob;
        uploadBlob = null;
        if (frameUrl) URL.revokeObjectURL(frameUrl);
        frameUrl = URL.createObjectURL(newFrameBlob);
        const img = await loadImage(frameUrl);
        if (gen !== generation) return false;
        frameImg = img;
        controller.setBaseSource(frameImg); // 点を打つまでは元のフレームをそのまま見せる
        controller.loadOriginalImage(frameUrl, () => gen !== generation);
      }
      jobId = newJobId;
      timeMs = newTimeMs;
      await registerFrame(gen);
      if (gen !== generation) return false;
      registered = true;
      setStatus(points.length === 0 ? '残したい所をタップしてください' : '');
      if (points.length > 0) {
        // やり直し（登録の失敗・計算の失敗の後）: 今の一覧を計算し直す
        shown = { version: -1, objectPresent: null };
        requestPreview();
      }
      sync();
      return true;
    } catch (err) {
      if (gen !== generation) return false;
      setStatus('');
      onError(err, 'プレビューの準備');
      sync();
      return false;
    }
  }

  function handlePointAdd(point) {
    if (locked) return;
    if (!registered) {
      setStatus('準備中です。少しお待ちください');
      return;
    }
    if (points.length >= MAX_POINTS) {
      setStatus(`点は ${MAX_POINTS} 個までです。Undo で減らしてから打ち直してください`);
      return;
    }
    points = [...points, point];
    redoStack = [];
    pointsChanged();
  }

  function pointsChanged() {
    version += 1;
    controller.setPoints(points);
    if (points.length === 0) {
      // 点が無いときは送らない（サーバーは 1〜8 個）。元のフレームに戻す
      controller.setBaseSource(frameImg);
      shown = { version, objectPresent: null };
      setStatus('残したい所をタップしてください');
      sync();
      return;
    }
    requestPreview();
    sync();
  }

  /** 今の点の一覧のマスクを取りに行く。応答待ちの間に一覧が変わったら、最新の一覧でもう一度送る */
  async function requestPreview() {
    if (computing) return; // 応答が来たら下のループが最新の一覧で送り直す
    const gen = generation;
    computing = true;
    sync();
    try {
      while (gen === generation && registered && points.length > 0 && !isCurrent()) {
        const sentVersion = version;
        setStatus('切り抜きを計算中…');
        try {
          const { maskBlob, objectPresent } = await fetchMask(videoPoints(), gen);
          if (gen !== generation || sentVersion !== version) continue;
          const maskImg = await loadImageFromBlob(maskBlob);
          if (gen !== generation || sentVersion !== version) continue;
          controller.setBaseSource(composeCutout(cutoutCanvas, frameImg, maskImg));
          shown = { version: sentVersion, objectPresent };
          setStatus(''); // 対象が見つからないときは warningEl を出す（sync）
        } catch (err) {
          if (gen !== generation || sentVersion !== version) continue;
          shown = { version: sentVersion, objectPresent: null }; // この一覧はもう送らない（点を変えるか、やり直しで送る）
          setStatus('');
          onError(err, 'プレビューの計算');
        }
      }
    } finally {
      if (gen === generation) {
        computing = false;
        sync();
      }
    }
  }

  /** /preview。フレームの特徴量が消えていたら登録し直して 1 回だけやり直す */
  async function fetchMask(sentPoints, gen) {
    const call = () => withBusyRetry(() => previewVideoMask(jobId, timeMs, sentPoints), gen);
    try {
      return await call();
    } catch (err) {
      if (err.code !== 'PREVIEW_FRAME_NOT_FOUND' || gen !== generation) throw err;
      setStatus('プレビューを準備し直しています…');
      await registerFrame(gen);
      if (gen !== generation) throw err;
      setStatus('切り抜きを計算中…');
      return call();
    }
  }

  /** 動画ジョブに送る点（Undo / Redo を反映した今の一覧、0〜1 の正規化座標） */
  function videoPoints() {
    if (!frameImg) return [];
    return points.map((p) => ({
      ...toNormalized(p.x, p.y, frameImg.naturalWidth, frameImg.naturalHeight),
      label: p.label === 1 ? 'include' : 'exclude',
    }));
  }

  /** 送信してよいか: 登録済み・点が 1 つ以上・今の一覧のマスクが出ていて対象が見つかっている */
  function canSend() {
    return registered && !computing && points.length > 0 && isCurrent() && shown.objectPresent === true;
  }

  /** 今の一覧のマスクで、対象が見つからなかった */
  function objectMissing() {
    return isCurrent() && points.length > 0 && shown.objectPresent === false;
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
    jobId = null;
    frameBlob = null;
    if (frameUrl) URL.revokeObjectURL(frameUrl);
    frameUrl = null;
    frameImg = null;
    uploadBlob = null;
    cutoutCanvas.width = 0; // 前の動画の切り抜きのメモリを手放す
    cutoutCanvas.height = 0;
    registered = false;
    points = [];
    redoStack = [];
    version = 0;
    shown = { version: 0, objectPresent: null };
    computing = false;
    locked = false;
    controller.reset();
    applyMode('positive');
    setStatus('');
    sync();
  }

  return {
    load,
    setActive,
    timeMs: () => timeMs,
    lock,
    unlock,
    reset,
    videoPoints,
    canSend,
    objectMissing,
    pointCount: () => points.length,
  };
}
