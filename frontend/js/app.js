/**
 * app.html のエントリーポイント。
 *
 * やることは配線だけ: store（状態）を1つ作り、strip（上部の画像一覧）と
 * editor（下の編集キャンバス）に渡す。strip で画像が選ばれたら editor.open を呼ぶ、という一方向の流れ。
 * 起動時に、リロード前の写真をブラウザ内の保存(js/persist.js)から復元し、
 * ホーム画面で選ばれた写真があれば strip に流し込む。各モジュールの詳細は frontend/README.md を参照。
 */
import { createStore } from './store.js?v=1.5.3';
import { initStrip } from './strip.js?v=1.5.3';
import { initEditor } from './editor.js?v=1.5.3';
import { takeFiles } from './handoff.js?v=1.5.3';
import { showToast } from './toast.js?v=1.5.3';
import { loadSaved, startAutoSave, isLikelyExpired } from './persist.js?v=1.5.3';

const store = createStore();

const editor = initEditor({
  store,
  elements: {
    canvasEl: document.getElementById('edit-canvas'),
    modeButtons: [document.getElementById('mode-keep'), document.getElementById('mode-remove')],
    undoBtn: document.getElementById('undo-btn'),
    redoBtn: document.getElementById('redo-btn'),
    saveBtn: document.getElementById('save-btn'),
    statusEl: document.getElementById('edit-status'),
    emptyHintEl: document.getElementById('empty-hint'),
    editorSectionEl: document.getElementById('editor'),
  },
});

const strip = initStrip({
  store,
  dropzoneEl: document.getElementById('dropzone'),
  stripWrapEl: document.getElementById('strip-wrap'),
  stripEl: document.getElementById('strip'),
  addBtnEl: document.getElementById('strip-add'),
  fileInputEl: document.getElementById('file-input'),
  prevBtnEl: document.getElementById('strip-prev'),
  nextBtnEl: document.getElementById('strip-next'),
  clearSavedBtnEl: document.getElementById('strip-clear-saved'),
  onSelect: (localId) => editor.open(localId),
  onClear: () => editor.close(),
});

start();

async function start() {
  // 1. リロード前の写真を復元する（サーバー側で期限切れのものは、開いたときに送り直す）
  const { records, activeLocalId } = await loadSaved();
  for (const rec of records) {
    const expired =
      !rec.sessionId || rec.status === 'uploading' || rec.status === 'expired' || isLikelyExpired(rec.touchedAt);
    store.createSession(rec.localId, {
      file: rec.file,
      thumbUrl: URL.createObjectURL(rec.file),
      sessionId: rec.sessionId,
      imageId: rec.imageId,
      points: rec.points || [],
      redoStack: rec.redoStack || [],
      touchedAt: rec.touchedAt,
      createdAt: rec.createdAt,
      status: expired ? 'expired' : rec.status,
    });
  }
  // 復元が終わってから保存を始める（復元途中の状態を書き戻さない）
  startAutoSave(store);

  // 2. ホーム画面で選ばれた写真があれば、そのままアップロードを始める（js/handoff.js）。
  //    その場合は新しい写真が自動で開くので、前回開いていた写真は開かない
  const files = await takeFiles();
  if (files.length > 0) {
    strip.handleFiles(files);
  } else if (activeLocalId) {
    strip.select(activeLocalId);
  }
}

// ホーム画面からの受け渡しに失敗した場合は、ここで選び直してもらう
const params = new URLSearchParams(location.search);
if (params.get('handoff') === 'failed') {
  showToast({
    type: 'warning',
    message: 'ホーム画面で選んだ写真を受け取れませんでした。ここでもう一度選んでください。',
  });
  history.replaceState(null, '', location.pathname);
}
