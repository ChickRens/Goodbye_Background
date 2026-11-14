import {
  getPreviewImage,
  segmentImage,
  undoEdit,
  redoEdit,
  saveImage,
  getCompletedImage,
  uploadImage,
} from './api.js?v=1.5.3';
import { createCanvasController } from './canvas.js?v=1.5.3';
import { showToast } from './toast.js?v=1.5.3';

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
    modeButtons,
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

  // 処理中の表示文言。空のときは renderStatus が「次にすること」の案内を代わりに出す
  let statusText = '';

  // 編集中の操作(タップ・元に戻す等)が期限切れで失敗した写真。送り直したときにその旨を伝える
  const expiredDuringEdit = new Set();

  store.subscribe((state) => {
    if (currentLocalId) syncToolbar(state.sessions.get(currentLocalId));
    renderStatus(); // ポイントの増減で案内の出し分けが変わる
  });

  for (const btn of modeButtons) {
    btn.addEventListener('click', () => applyMode(btn.dataset.mode));
  }

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

    let session = store.getSession(localId);
    syncToolbar(session);
    if (!session) return;
    if (session.status === 'uploading') {
      showToast({ type: 'warning', message: 'この画像はまだアップロードが完了していません' });
      return;
    }

    emptyHintEl.hidden = true;
    editorSectionEl.hidden = false;

    // サーバー側で期限切れになっている写真は、保存しておいた元ファイルで送り直してから開く
    if (session.status === 'expired' || !session.sessionId) {
      const ok = await reupload(localId);
      if (!ok || localId !== currentLocalId) return;
      session = store.getSession(localId);
    }

    setStatus('画像を読み込み中…');

    let url = null;
    let adopted = false;
    try {
      try {
        url = await getPreviewImage(session.imageId);
      } catch (err) {
        // 期限内のつもりでもサーバーの再起動などで消えていることがある。そのときも送り直す
        if (err.status !== 404) throw err;
        const ok = await reupload(localId);
        if (!ok || localId !== currentLocalId) return;
        url = await getPreviewImage(store.getSession(localId).imageId);
      }
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
    for (const btn of modeButtons) {
      btn.setAttribute('aria-pressed', String(btn.dataset.mode === mode));
    }
    // 親(.mode-switch)の data-mode で、選択中を示す色の板をスライドさせる（css/style.css）
    modeButtons[0].parentElement.dataset.mode = mode;
    controller.setPointMode(mode);
  }

  function handlePointAdd(point) {
    if (!currentLocalId) return;
    const localId = currentLocalId;
    const session = store.getSession(localId);
    if (!session) return;

    if (session.busy) {
      // 処理中の変更系リクエストと重ねて送らない。タップ自体を無視する。
      setStatus('切り抜きが終わってからタップしてください');
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
    setStatus('切り抜き中…');
    let segmentApplied = false;
    try {
      const { imageId } = await segmentImage(session.sessionId, point);
      segmentApplied = true; // ここから先の失敗はサーバー側では適用済み。マーカーは戻さない
      store.updateSession(localId, { imageId, touchedAt: Date.now() });

      const url = await getPreviewImage(imageId);
      replaceFullUrl(localId, url);

      // 待っている間に別の画像に切り替えられていたら、今開いている画面には反映しない
      if (localId === currentLocalId) {
        await controller.loadBaseImage(url, isStale(localId));
        setStatus('');
      }
    } catch (err) {
      if (!segmentApplied) rollbackPoint(localId, point, previousRedoStack);
      if (err.status === 404) {
        handleExpired(localId);
      } else if (localId === currentLocalId) {
        setStatus('');
        showToast({ type: 'danger', message: `切り抜きに失敗しました: ${err.message}`, code: err.code });
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
    setStatus('切り抜きを更新中…');
    try {
      const { imageId } = await apiFn(session.sessionId);
      // サーバーの履歴は進んだので、表示中かどうかに関わらずマーカー履歴も同期する
      store.updateSession(localId, { imageId, touchedAt: Date.now() });
      localPointSync(store.getSession(localId));

      const url = await getPreviewImage(imageId);
      replaceFullUrl(localId, url);

      if (localId === currentLocalId) {
        await controller.loadBaseImage(url, isStale(localId));
        setStatus('');
      }
    } catch (err) {
      if (err.status === 404) {
        handleExpired(localId);
      } else if (localId === currentLocalId) {
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
    setStatus('保存用の画像を作成中…（元の解像度で作るので少し時間がかかります）');
    try {
      const { imageId: completedImageId } = await saveImage(session.sessionId);
      store.updateSession(localId, { touchedAt: Date.now() });
      setStatus('ダウンロード準備中…');
      const blob = await getCompletedImage(completedImageId);

      // 保存中に別の画像へ切り替えられていても、ファイルはできているのでダウンロードする
      store.updateSession(localId, { status: 'saved' });
      if (localId === currentLocalId) setStatus('');
      showToast({ type: 'success', message: fileName ? `${fileName} の保存が完了しました` : '保存が完了しました' });
      triggerDownload(blob, fileName);
    } catch (err) {
      if (localId === currentLocalId) setStatus('');
      if (err.status === 404) {
        store.updateSession(localId, { status: 'ready' });
        handleExpired(localId);
        return;
      }
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

  /**
   * サーバー側で期限切れになった写真を、保存しておいた元ファイルで送り直す。
   * サーバーの編集状態は消えているので、タップした点と履歴はリセットされる。
   * @returns {Promise<boolean>} 送り直せたら true
   */
  async function reupload(localId) {
    const session = store.getSession(localId);
    if (!session || !session.file || session.busy) return false;

    const hadPoints = session.points.length > 0;
    // 編集中の操作が期限切れで失敗した場合（handleExpired）は、その操作が反映されていないことを必ず伝える
    const lostAction = expiredDuringEdit.delete(localId);
    store.updateSession(localId, { busy: true });
    if (localId === currentLocalId) setStatus('時間が経ったため、写真を送り直しています…');
    try {
      const { sessionId, imageId } = await uploadImage(session.file);
      store.updateSession(localId, {
        sessionId,
        imageId,
        status: 'ready',
        points: [],
        redoStack: [],
        touchedAt: Date.now(),
      });
      if (localId === currentLocalId) {
        controller.setPoints([]);
        setStatus('');
      }
      if (hadPoints || lostAction) {
        showToast({
          type: 'info',
          message: hadPoints
            ? '時間が経ってサーバーから消えていたため、写真を送り直しました。タップした点はリセットされています。'
            : '時間が経ってサーバーから消えていたため、写真を送り直しました。もう一度タップしてください。',
        });
      }
      return true;
    } catch (err) {
      store.updateSession(localId, { status: 'expired' });
      if (localId === currentLocalId) {
        setStatus('');
        showToast({ type: 'danger', message: `写真を送り直せませんでした: ${err.message}`, code: err.code });
      }
      return false;
    } finally {
      store.updateSession(localId, { busy: false });
    }
  }

  // 編集中にサーバーから「見つからない(404)」が返った = 期限切れ。送り直して開き直す
  function handleExpired(localId) {
    expiredDuringEdit.add(localId);
    store.updateSession(localId, { status: 'expired' });
    if (localId === currentLocalId) {
      setStatus('');
      // busy の解除(finally)を待ってから開き直す
      setTimeout(() => { if (localId === currentLocalId) open(localId); }, 0);
    }
  }

  function setStatus(text) {
    statusText = text;
    renderStatus();
  }

  // 処理中はその内容を、何もしていないときは「次にすること」を画像の上に出す。
  // 1点目を置いたあとは案内を消して、画像を見やすくする。
  function renderStatus() {
    let text = statusText;
    if (!text) {
      const session = currentLocalId ? store.getSession(currentLocalId) : null;
      if (session && session.status !== 'saving' && session.points.length === 0) {
        text = '残したいところをタップしてください';
      }
    }
    if (statusEl.textContent !== text) statusEl.textContent = text;
  }

  /** 開いている写真が削除されて、開くものが無くなったときに編集画面を閉じる */
  function close() {
    currentLocalId = null;
    controller.reset();
    statusText = '';
    statusEl.textContent = '';
    editorSectionEl.hidden = true;
    emptyHintEl.hidden = false;
  }

  return { open, close };
}