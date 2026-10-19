import { getPreviewImage, segmentImage, undoEdit, redoEdit, saveImage, getCompletedImage } from './api.js';
import { createCanvasController } from './canvas.js';
import { showToast } from './toast.js';

/**
 * 画面には常に1枚分の大きな編集エリア（canvas+ツールバー）だけがあり、
 * 上部のストリップ（strip.js）でどのセッションを表示するかを切り替える。
 *
 * image_id はスナップショットのポインタで、segment/undo/redo のたびに
 * 新しい image_id が発行される。実質的にversionの役割を果たすが、
 * 複数の変更系リクエストが同時に飛んだ場合「どちらのimage_idが
 * 最終的に正しいか」はフロント側では判定できない。そのため
 * session.busy をロックとして使い、変更系リクエスト
 * (segment / undo / redo / save) を同時に2つ以上飛ばさないようにする。
 *
 * また、ストリップで別の画像に切り替えた直後に、前の画像への
 * 非同期処理（segment等）の応答が遅れて届くことがある。その場合
 * 「呼び出し時点の localId」と「今表示している currentLocalId」を
 * 比較し、一致しなければ描画を破棄する。これをしないと、切り替え後の
 * 画面に前の画像の結果が紛れ込むレースコンディションが起きる。
 * 画像の切り替え時は canvas を一度空にして(controller.reset)、読み込みが
 * 終わるまで前の画像が残ったままタップできてしまうのを防ぐ。
 *
 * undo/redoはサーバーがポイント一覧を返さない(image_idだけを返す)ため、
 * マーカー表示用の points 配列はクライアント側で
 * 「1回のundo = 直前に追加した1点を取り除く」
 * 「1回のredo = 取り除いた1点を戻す」
 * という前提でベストエフォートに同期する。実際に表示される画像自体は
 * 常に get-preview で取得し直すので、この前提が崩れてもマスクの表示は
 * 正しいままだが、マーカーの見た目だけはズレ得る。
 * segment が失敗した場合は、先に追加しておいた点を取り除いて元に戻す。
 *
 * /get-preview の結果は Blob URL。差し替えるたびに古いURLを revoke する
 * （しないと操作のたびに画像がメモリに溜まる）。
 */

// サーバーが返す形式(現状 image/webp)に合わせた拡張子。中身と拡張子を一致させる。
const EXTENSION_BY_MIME = {
  'image/webp': 'webp',
  'image/png': 'png',
  'image/jpeg': 'jpg',
};

export function initEditor({ store, elements }) {
  const {
    canvasEl,
    modeToggleBtn,
    undoBtn,
    redoBtn,
    saveBtn,
    statusEl,
    emptyHintEl,
    editorSectionEl,
  } = elements;

  let currentLocalId = null;

  const controller = createCanvasController(canvasEl, {
    onPointAdd: handlePointAdd,
  });

  store.subscribe((state) => {
    if (currentLocalId) syncToolbar(state.sessions.get(currentLocalId));
  });

  modeToggleBtn.addEventListener('click', () => {
    const next = modeToggleBtn.dataset.mode === 'positive' ? 'negative' : 'positive';
    applyMode(next);
  });

  undoBtn.addEventListener('click', () => runHistory(undoEdit, popLastPoint));
  redoBtn.addEventListener('click', () => runHistory(redoEdit, restoreLastPoint));
  saveBtn.addEventListener('click', () => runSave());

  // Ctrl/Cmd+Z で Undo、Ctrl/Cmd+Shift+Z または Ctrl+Y で Redo。
  // 無効状態のボタンへの click() は何も起きないので、busy中や履歴が空のときは自然に無視される。
  document.addEventListener('keydown', (evt) => {
    if (!(evt.ctrlKey || evt.metaKey) || evt.altKey) return;
    const target = evt.target;
    if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)) {
      return;
    }
    const key = evt.key.toLowerCase();
    if (key === 'z') {
      evt.preventDefault();
      (evt.shiftKey ? redoBtn : undoBtn).click();
    } else if (key === 'y') {
      evt.preventDefault();
      redoBtn.click();
    }
  });

  applyMode('positive');

  const isStale = (localId) => () => localId !== currentLocalId;

  /**
   * session.fullUrl を差し替え、古いBlob URLを解放する。
   * canvas側の Image は読み込み済み(デコード済み)なので、URLを解放しても表示は消えない。
   */
  function replaceFullUrl(localId, url) {
    const session = store.getSession(localId);
    if (session && session.fullUrl && session.fullUrl !== url) {
      URL.revokeObjectURL(session.fullUrl);
    }
    store.updateSession(localId, { fullUrl: url });
  }

  async function open(localId) {
    currentLocalId = localId;
    controller.reset(); // 前の画像が残ったままタップできてしまわないよう、まず空にする

    const session = store.getSession(localId);
    syncToolbar(session);
    if (!session || !session.imageId) {
      showToast({ type: 'warning', message: 'この画像はまだアップロードが完了していません' });
      return;
    }

    emptyHintEl.hidden = true;
    editorSectionEl.hidden = false;

    setStatus('画像を読み込み中…');

    let url = null;
    let adopted = false;
    try {
      url = await getPreviewImage(session.imageId);
      if (localId !== currentLocalId) return; // その間に別画像へ切り替えられていたら破棄

      const size = await controller.loadBaseImage(url, isStale(localId));
      if (!size) return; // 読み込み中に切り替えられた

      replaceFullUrl(localId, url);
      adopted = true;
      store.updateSession(localId, {
        status: 'editing',
        naturalWidth: size.width,
        naturalHeight: size.height,
      });

      const latest = store.getSession(localId);
      controller.setPoints(latest.points);
      // 元画像の薄い表示（既にあるBlob URLを再利用、通信不要）。待たずに進める
      controller.loadOriginalImage(latest.thumbUrl, isStale(localId));
      setStatus('');
    } catch (err) {
      if (localId === currentLocalId) {
        setStatus('');
        showToast({ type: 'danger', message: `読み込みに失敗しました: ${err.message}`, code: err.code });
      }
    } finally {
      // 採用されなかったBlob URL（切り替えで破棄 / 失敗）は解放する
      if (url && !adopted) URL.revokeObjectURL(url);
    }
  }

  function applyMode(mode) {
    modeToggleBtn.dataset.mode = mode;
    modeToggleBtn.textContent = mode === 'positive' ? '➕ 含める（タップで追加）' : '➖ 除外する（タップで追加）';
    controller.setPointMode(mode);
  }

  function handlePointAdd(point) {
    if (!currentLocalId) return;
    const localId = currentLocalId;
    const session = store.getSession(localId);
    if (!session) return;

    if (session.busy) {
      // 処理中の変更系リクエストと重ねて送らない。タップ自体を無視する。
      setStatus('処理中はポイントを追加できません。完了までお待ちください');
      return;
    }

    // 新しくポイントを打ったらredoの前提が崩れるのでredoStackは破棄する。
    // ただし segment が失敗したら元に戻すので、破棄前の内容を渡しておく。
    const previousRedoStack = session.redoStack;
    const points = [...session.points, point];
    store.updateSession(localId, { points, redoStack: [] });
    controller.setPoints(points);
    runSegment(localId, point, previousRedoStack);
  }

  /** segment が失敗したとき、先に追加しておいたマーカーを取り除いて元の状態に戻す */
  function rollbackPoint(localId, point, previousRedoStack) {
    const session = store.getSession(localId);
    if (!session) return;
    const index = session.points.lastIndexOf(point);
    if (index === -1) return;
    const points = session.points.filter((_, i) => i !== index);
    store.updateSession(localId, { points, redoStack: previousRedoStack });
    if (localId === currentLocalId) controller.setPoints(points);
  }

  async function runSegment(localId, point, previousRedoStack) {
    const session = store.getSession(localId);
    if (!session || session.busy) {
      rollbackPoint(localId, point, previousRedoStack);
      return;
    }

    store.updateSession(localId, { busy: true });
    setStatus('セグメンテーション実行中…');
    let segmentApplied = false;
    try {
      const { imageId } = await segmentImage(session.sessionId, point);
      segmentApplied = true; // ここから先の失敗はサーバー側では適用済み。マーカーは戻さない
      store.updateSession(localId, { imageId });

      const url = await getPreviewImage(imageId);
      replaceFullUrl(localId, url);

      // 待っている間に別の画像に切り替えられていたら、今開いている画面には反映しない
      if (localId === currentLocalId) {
        await controller.loadBaseImage(url, isStale(localId));
        setStatus('');
      }
    } catch (err) {
      if (!segmentApplied) rollbackPoint(localId, point, previousRedoStack);
      if (localId === currentLocalId) {
        setStatus('');
        showToast({ type: 'danger', message: `セグメンテーションに失敗しました: ${err.message}`, code: err.code });
      }
    } finally {
      store.updateSession(localId, { busy: false });
    }
  }

  function popLastPoint(session) {
    if (session.points.length === 0) return;
    const points = session.points.slice(0, -1);
    const removed = session.points[session.points.length - 1];
    const redoStack = [...session.redoStack, removed];
    store.updateSession(session.localId, { points, redoStack });
    if (session.localId === currentLocalId) controller.setPoints(points);
  }

  function restoreLastPoint(session) {
    if (session.redoStack.length === 0) return;
    const redoStack = session.redoStack.slice(0, -1);
    const restored = session.redoStack[session.redoStack.length - 1];
    const points = [...session.points, restored];
    store.updateSession(session.localId, { points, redoStack });
    if (session.localId === currentLocalId) controller.setPoints(points);
  }

  async function runHistory(apiFn, localPointSync) {
    const localId = currentLocalId;
    const session = store.getSession(localId);
    if (!session || session.busy) return;

    store.updateSession(localId, { busy: true });
    setStatus('処理中…');
    try {
      const { imageId } = await apiFn(session.sessionId);
      // サーバーの履歴は進んだので、表示中かどうかに関わらずマーカー履歴も同期する
      store.updateSession(localId, { imageId });
      localPointSync(store.getSession(localId));

      const url = await getPreviewImage(imageId);
      replaceFullUrl(localId, url);

      if (localId === currentLocalId) {
        await controller.loadBaseImage(url, isStale(localId));
        setStatus('');
      }
    } catch (err) {
      if (localId === currentLocalId) {
        setStatus('');
        showToast({ type: 'danger', message: `失敗しました: ${err.message}`, code: err.code });
      }
    } finally {
      store.updateSession(localId, { busy: false });
    }
  }

  function syncToolbar(session) {
    const busy = !session || session.busy;
    // 戻せる/やり直せる履歴が無いときは押せないようにする
    undoBtn.disabled = busy || session.points.length === 0;
    redoBtn.disabled = busy || session.redoStack.length === 0;
    saveBtn.disabled = busy;
  }

  async function runSave() {
    const localId = currentLocalId;
    const session = store.getSession(localId);
    if (!session || session.busy) return;

    const fileName = session.file ? session.file.name : null;
    store.updateSession(localId, { busy: true, status: 'saving' });
    setStatus('保存用のフル画質版を作成中…');
    try {
      const { imageId: completedImageId } = await saveImage(session.sessionId);
      setStatus('ダウンロード準備中…');
      const blob = await getCompletedImage(completedImageId);

      // 保存中に別の画像へ切り替えられていても、ファイルはできているのでダウンロードする
      store.updateSession(localId, { status: 'saved' });
      if (localId === currentLocalId) setStatus('');
      showToast({ type: 'success', message: fileName ? `${fileName} の保存が完了しました` : '保存が完了しました' });
      triggerDownload(blob, fileName);
    } catch (err) {
      if (localId === currentLocalId) setStatus('');
      showToast({
        type: 'danger',
        message: `${fileName ? `${fileName}: ` : ''}保存に失敗しました: ${err.message}`,
        code: err.code,
      });
      store.updateSession(localId, { status: 'ready' }); // 失敗したら状態を戻す
    } finally {
      store.updateSession(localId, { busy: false });
    }
  }

  function triggerDownload(blob, originalFileName) {
    const url = URL.createObjectURL(blob);
    const baseName = originalFileName ? originalFileName.replace(/\.[^./\\]+$/, '') : 'image';
    // 拡張子はサーバーが返した実際の形式に合わせる（中身がWebPなのに .png にしない）
    const extension = EXTENSION_BY_MIME[blob.type] || 'webp';
    const a = document.createElement('a');
    a.href = url;
    a.download = `${baseName}_cropped.${extension}`;
    document.body.appendChild(a);
    a.click();
    a.remove();
    // ダウンロード開始直後に取り消すとブラウザによっては失敗するので少し待つ
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  function setStatus(text) {
    statusEl.textContent = text;
  }

  return { open };
}