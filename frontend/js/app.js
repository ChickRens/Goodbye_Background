/**
 * app.html のエントリーポイント。
 *
 * やることは配線だけ: store（状態）を1つ作り、strip（上部の画像一覧）と
 * editor（下の編集キャンバス）に渡す。strip で画像が選ばれたら editor.open を呼ぶ、
 * という一方向の流れ。各モジュールの詳細は frontend/README.md を参照。
 */
import { createStore } from './store.js';
import { initStrip } from './strip.js';
import { initEditor } from './editor.js';

const store = createStore();

const editor = initEditor({
  store,
  elements: {
    canvasEl: document.getElementById('edit-canvas'),
    modeToggleBtn: document.getElementById('mode-toggle'),
    undoBtn: document.getElementById('undo-btn'),
    redoBtn: document.getElementById('redo-btn'),
    saveBtn: document.getElementById('save-btn'),
    statusEl: document.getElementById('edit-status'),
    emptyHintEl: document.getElementById('empty-hint'),
    editorSectionEl: document.getElementById('editor'),
  },
});

initStrip({
  store,
  dropzoneEl: document.getElementById('dropzone'),
  stripWrapEl: document.getElementById('strip-wrap'),
  stripEl: document.getElementById('strip'),
  addBtnEl: document.getElementById('strip-add'),
  fileInputEl: document.getElementById('file-input'),
  prevBtnEl: document.getElementById('strip-prev'),
  nextBtnEl: document.getElementById('strip-next'),
  onSelect: (localId) => editor.open(localId),
});