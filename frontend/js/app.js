import { createStore } from './store.js';
import { initStrip } from './strip.js';
import { initEditor } from './editor.js';
import { initAuth } from './auth.js';

const store = createStore();

initAuth({
  store,
  loginLinkEl: document.getElementById('login-link'),
  userEl: document.getElementById('auth-user'),
  userNameEl: document.getElementById('auth-user-name'),
  logoutBtnEl: document.getElementById('logout-btn'),
  deleteAccountBtnEl: document.getElementById('delete-account-btn'),
});

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