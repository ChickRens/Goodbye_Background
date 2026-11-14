/**
 * ホーム画面(index.html)の写真選択エリア。
 * 選ばれた(またはドロップされた)画像を handoff.js でブラウザ内に一時保存し、アプリ画面へ移動する。
 * アプリ画面(js/app.js)が起動時に取り出してアップロードを始める。
 */
import { saveFiles } from './handoff.js?v=1.5.2';

const zone = document.getElementById('start-zone');
const input = document.getElementById('start-input');
const errorEl = document.getElementById('start-error');

async function start(fileList) {
  const files = Array.from(fileList || []).filter((f) => f.type.startsWith('image/'));
  if (files.length === 0) {
    errorEl.textContent = '画像ファイルを選んでください（JPEG・PNG・WebP など）。';
    errorEl.hidden = false;
    return;
  }
  errorEl.hidden = true;
  zone.classList.add('is-busy');
  const saved = await saveFiles(files);
  // 受け渡しに失敗したら、アプリ画面でその旨を伝えて選び直してもらう
  location.href = saved ? 'app.html' : 'app.html?handoff=failed';
}

input.addEventListener('change', () => {
  start(input.files);
  input.value = ''; // 戻るボタンで戻ってきたとき、同じ写真をもう一度選べるようにする
});

zone.addEventListener('dragover', (evt) => {
  evt.preventDefault();
  zone.classList.add('is-dragover');
});
zone.addEventListener('dragleave', () => zone.classList.remove('is-dragover'));
zone.addEventListener('drop', (evt) => {
  evt.preventDefault();
  zone.classList.remove('is-dragover');
  start(evt.dataTransfer.files);
});

// 戻るボタンでキャッシュから表示されたとき、処理中の表示を消す
window.addEventListener('pageshow', () => zone.classList.remove('is-busy'));
