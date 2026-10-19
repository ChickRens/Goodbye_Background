/**
 * canvas 1枚で「画像の表示」「透過領域を示す市松模様」「元画像の薄い表示」
 * 「ポイントの入力/描画」をすべて担当する。
 *
 * 描画順（下から上）:
 *   1. 市松模様（透明度が2秒周期で脈動する。まだ切り抜けていない領域の目印）
 *   2. 元画像を半透明で表示（session.file のBlob URLをそのまま使う。ネットワーク不要）
 *   3. サーバーから返るプレビュー画像（切り抜き済みの部分は不透明なので1,2を覆い隠す）
 *   4. ポイントのマーカー
 *
 * マウスとタッチを別々に扱うと（click + touchstart のように）、
 * ブラウザによってはタッチ後に合成クリックが発生して二重登録になる。
 * Pointer Events (pointerdown) に一本化することでその問題を避ける。
 * さらに、右クリックや2本目以降の指(マルチタッチ)ではポイントを打たない。
 *
 * canvas自体の解像度(width/height)は「画像の実ピクセルサイズ」に固定し、
 * CSS側で表示サイズを可変にする。ポイント座標は常に画像本来のピクセル
 * 座標系で保持・送信する（表示倍率に依存させない）。
 *
 * 描画負荷の抑え方:
 *   - 脈動アニメーションは約30fpsに間引く（大きい画像の全面再描画を毎フレームやらない）
 *   - 「動きを減らす」設定(prefers-reduced-motion)のときは脈動を止め、
 *     画像・ポイントが変わったときだけ描画する
 *   - CSS変数の色は最初に1回だけ読む（毎フレーム getComputedStyle を呼ばない）
 */

const CHECKER_TILE_SIZE = 20; // 画面上(CSSピクセル)での1マスのサイズ。画像の縮小表示に合わせて補正する
const CHECKER_MIN_ALPHA = 0.15;
const CHECKER_MAX_ALPHA = 0.55;
const CHECKER_PERIOD_MS = 2000; // 「2秒間隔」で1周期の脈動
const ORIGINAL_GHOST_ALPHA = 0.35;
const FRAME_INTERVAL_MS = 1000 / 30;

export function createCanvasController(canvasEl, { onPointAdd }) {
  const ctx = canvasEl.getContext('2d');
  const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)');

  let baseImage = null; // サーバーから返るプレビュー画像
  let originalImage = null; // アップロード直後のFileから作った元画像（薄く表示する用）
  let points = [];
  let pointMode = 'positive'; // 'positive' | 'negative'

  let checkerPattern = null;
  let checkerTilePx = 0;
  let colors = null;

  let dirty = true; // 再描画が必要（動きを減らす設定のときはこれが立ったときだけ描く）
  let lastDrawAt = -Infinity;

  function setPointMode(mode) {
    pointMode = mode;
  }

  function getColors() {
    if (!colors) {
      const style = getComputedStyle(document.documentElement);
      const read = (name) => style.getPropertyValue(name).trim();
      colors = {
        checkerLight: read('--checker-light'),
        checkerDark: read('--checker-dark'),
        success: read('--success'),
        danger: read('--danger'),
        outline: read('--bg-dark'),
      };
    }
    return colors;
  }

  /**
   * 表示中の画像を破棄して空にする。画像の切り替え直後に、前の画像が残ったまま
   * 操作できてしまうのを防ぐ（baseImage が無い間はタップも無視される）。
   */
  function reset() {
    baseImage = null;
    originalImage = null;
    points = [];
    dirty = true;
  }

  /**
   * @param {string} url
   * @param {() => boolean} [isStale] 読み込み完了時点で true なら、結果を捨てて null を返す
   *   （読み込み中に別の画像へ切り替えられた場合に、古い画像が紛れ込まないようにする）
   */
  function loadBaseImage(url, isStale) {
    return new Promise((resolve, reject) => {
      const img = new Image();
      img.onload = () => {
        if (isStale && isStale()) {
          resolve(null);
          return;
        }
        baseImage = img;
        canvasEl.width = img.naturalWidth;
        canvasEl.height = img.naturalHeight;
        dirty = true;
        resolve({ width: img.naturalWidth, height: img.naturalHeight });
      };
      img.onerror = () => reject(new Error('画像の読み込みに失敗しました'));
      img.src = url;
    });
  }

  function loadOriginalImage(url, isStale) {
    if (!url) {
      originalImage = null;
      dirty = true;
      return Promise.resolve();
    }
    return new Promise((resolve) => {
      const img = new Image();
      img.onload = () => {
        if (!(isStale && isStale())) {
          originalImage = img;
          dirty = true;
        }
        resolve();
      };
      // 元画像は「あれば嬉しい」補助表示なので、読み込み失敗しても編集自体は止めない
      img.onerror = () => {
        originalImage = null;
        dirty = true;
        resolve();
      };
      img.src = url;
    });
  }

  function setPoints(newPoints) {
    points = newPoints;
    dirty = true;
  }

  // 画像を縮小表示しているときも、画面上で見た目が一定の大きさになるようにする
  function getCheckerPattern() {
    const displayScale = canvasEl.width / (canvasEl.clientWidth || canvasEl.width);
    const tilePx = Math.max(4, Math.round(CHECKER_TILE_SIZE * displayScale));
    if (checkerPattern && tilePx === checkerTilePx) return checkerPattern;

    const palette = getColors();
    const patternCanvas = document.createElement('canvas');
    patternCanvas.width = tilePx * 2;
    patternCanvas.height = tilePx * 2;
    const pctx = patternCanvas.getContext('2d');

    pctx.fillStyle = palette.checkerLight;
    pctx.fillRect(0, 0, tilePx * 2, tilePx * 2);
    pctx.fillStyle = palette.checkerDark;
    pctx.fillRect(0, 0, tilePx, tilePx);
    pctx.fillRect(tilePx, tilePx, tilePx, tilePx);

    checkerPattern = ctx.createPattern(patternCanvas, 'repeat');
    checkerTilePx = tilePx;
    return checkerPattern;
  }

  function checkerAlphaAtTime(nowMs) {
    if (reduceMotion.matches) {
      return (CHECKER_MIN_ALPHA + CHECKER_MAX_ALPHA) / 2; // 脈動しない固定値
    }
    const phase = ((nowMs % CHECKER_PERIOD_MS) / CHECKER_PERIOD_MS) * Math.PI * 2;
    const t = 0.5 + 0.5 * Math.sin(phase); // 0〜1をなめらかに往復
    return CHECKER_MIN_ALPHA + (CHECKER_MAX_ALPHA - CHECKER_MIN_ALPHA) * t;
  }

  function draw(nowMs = performance.now()) {
    ctx.clearRect(0, 0, canvasEl.width, canvasEl.height);
    if (!baseImage) return;

    // 1. 市松模様（脈動する透明度）
    ctx.save();
    ctx.globalAlpha = checkerAlphaAtTime(nowMs);
    ctx.fillStyle = getCheckerPattern();
    ctx.fillRect(0, 0, canvasEl.width, canvasEl.height);
    ctx.restore();

    // 2. 元画像を薄く表示
    if (originalImage) {
      ctx.save();
      ctx.globalAlpha = ORIGINAL_GHOST_ALPHA;
      ctx.drawImage(originalImage, 0, 0, canvasEl.width, canvasEl.height);
      ctx.restore();
    }

    // 3. プレビュー画像（不透明な部分は1,2を覆い隠す）
    ctx.drawImage(baseImage, 0, 0, canvasEl.width, canvasEl.height);

    // 4. ポイントのマーカー
    const palette = getColors();
    const radius = Math.max(5, canvasEl.width * 0.008);
    ctx.lineWidth = Math.max(1.5, radius * 0.25);
    ctx.strokeStyle = palette.outline;
    for (const p of points) {
      ctx.beginPath();
      ctx.arc(p.x, p.y, radius, 0, Math.PI * 2);
      ctx.fillStyle = p.label === 1 ? palette.success : palette.danger;
      ctx.fill();
      ctx.stroke();
    }
  }

  // 市松模様のアニメーション用の常時ループ。ただし実際に描くのは
  // 「アニメーションが有効」または「再描画が必要」なときだけ、かつ約30fpsまで。
  function animationLoop(nowMs) {
    const animated = !reduceMotion.matches;
    if ((animated || dirty) && nowMs - lastDrawAt >= FRAME_INTERVAL_MS) {
      lastDrawAt = nowMs;
      dirty = false;
      draw(nowMs);
    }
    requestAnimationFrame(animationLoop);
  }
  requestAnimationFrame(animationLoop);

  // 画面サイズが変わると市松模様の補正倍率も変わる
  window.addEventListener('resize', () => {
    dirty = true;
  });

  function coordsFromEvent(evt) {
    const rect = canvasEl.getBoundingClientRect();
    const scaleX = canvasEl.width / rect.width;
    const scaleY = canvasEl.height / rect.height;
    const x = (evt.clientX - rect.left) * scaleX;
    const y = (evt.clientY - rect.top) * scaleY;
    return {
      x: Math.max(0, Math.min(canvasEl.width, x)),
      y: Math.max(0, Math.min(canvasEl.height, y)),
    };
  }

  canvasEl.addEventListener('pointerdown', (evt) => {
    if (!baseImage) return;
    // マルチタッチの2本目以降、マウスの右/中ボタンは無視する
    if (!evt.isPrimary) return;
    if (evt.pointerType === 'mouse' && evt.button !== 0) return;
    evt.preventDefault();
    const { x, y } = coordsFromEvent(evt);
    onPointAdd({ x, y, label: pointMode === 'positive' ? 1 : 0 });
  });

  return { loadBaseImage, loadOriginalImage, setPoints, setPointMode, reset, draw };
}