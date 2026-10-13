import { uploadImage } from './api.js';
import { createLocalId } from './id.js';
import { showToast } from './toast.js';

/**
 * アップロードの入口は2つある。
 * - dropzoneEl: 画像が1枚も無い間だけ表示する大きいドロップゾーン
 * - addBtnEl（ストリップ内の「＋」タイル）: 画像が1枚でもあれば表示する、
 *   小さいストリップ側の入口
 * どちらも同じ handleFiles() に合流する。1枚でもセッションができた時点で
 * dropzoneEl を隠し、stripWrapEl（矢印＋サムネイル一覧）を表示する——
 * これにより「最初は大きく、画像が増えたら小さく」という見た目になる。
 *
 * サムネイルをクリックすると、そのセッションが編集画面（editor.js）に
 * 切り替わる（onSelectコールバック経由）。何も選ばれていない状態で
 * アップロードが完了した画像があれば、最初の1枚だけ自動的に選択する
 * （毎回のアップロードで編集中の画像を勝手に奪わないよう、既に何か
 * 選ばれていれば自動選択はしない）。
 */
export function initStrip({
  store,
  dropzoneEl,
  stripWrapEl,
  stripEl,
  addBtnEl,
  fileInputEl,
  prevBtnEl,
  nextBtnEl,
  onSelect,
}) {
  let activeLocalId = null;

  dropzoneEl.addEventListener('click', () => fileInputEl.click());
  addBtnEl.addEventListener('click', () => fileInputEl.click());

  fileInputEl.addEventListener('change', (evt) => {
    handleFiles(evt.target.files);
    fileInputEl.value = ''; // 同じファイルを連続選択できるようにリセット
  });

  // 大きいドロップゾーン・ストリップどちらにもドラッグ＆ドロップ対応（PC向け）
  for (const el of [dropzoneEl, stripEl]) {
    el.addEventListener('dragover', (evt) => {
      evt.preventDefault();
      el.classList.add('is-dragover');
    });
    el.addEventListener('dragleave', () => {
      el.classList.remove('is-dragover');
    });
    el.addEventListener('drop', (evt) => {
      evt.preventDefault();
      el.classList.remove('is-dragover');
      handleFiles(evt.dataTransfer.files);
    });
  }

  // PC向けの矢印ボタン。スマホは直接スワイプでスクロールできるので不要
  // （CSS側で `pointer: coarse` のときは非表示にしている）
  prevBtnEl.addEventListener('click', () => {
    stripEl.scrollBy({ left: -200, behavior: 'smooth' });
  });
  nextBtnEl.addEventListener('click', () => {
    stripEl.scrollBy({ left: 200, behavior: 'smooth' });
  });

  stripEl.addEventListener('click', (evt) => {
    const thumb = evt.target.closest('[data-local-id]');
    if (!thumb || thumb.disabled) return;
    setActive(thumb.dataset.localId);
  });

  store.subscribe((state) => render(state));
  render(store.getState());

  function handleFiles(fileList) {
    const files = Array.from(fileList || []).filter((f) => f.type.startsWith('image/'));
    for (const file of files) {
      const localId = createLocalId();
      const thumbUrl = URL.createObjectURL(file);

      store.createSession(localId, { thumbUrl, file });

      uploadImage(file)
        .then(({ sessionId, imageId }) => {
          store.updateSession(localId, { sessionId, imageId, status: 'ready' });
          if (!activeLocalId) setActive(localId);
        })
        .catch((err) => {
          store.updateSession(localId, { status: 'error', error: err.message });
          showToast({
            type: 'danger',
            message: `${file.name} のアップロードに失敗しました: ${err.message}`,
            code: err.code,
          });
        });
    }
  }

  function setActive(localId) {
    activeLocalId = localId;
    render(store.getState());
    onSelect(localId);
  }

  function render(state) {
    const hasImages = state.sessions.size > 0;
    dropzoneEl.hidden = hasImages;
    stripWrapEl.hidden = !hasImages;

    // addBtnEl以外のサムネイルを作り直す。枚数が多くない前提のシンプルな実装。
    stripEl.querySelectorAll('[data-local-id]').forEach((el) => el.remove());

    for (const session of state.sessions.values()) {
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'strip-thumb';
      btn.dataset.localId = session.localId;
      btn.disabled = session.status === 'uploading' || session.status === 'error';
      btn.classList.toggle('is-active', session.localId === activeLocalId);
      btn.title = session.file ? session.file.name : '画像';

      const img = document.createElement('img');
      img.src = session.thumbUrl;
      img.alt = session.file ? session.file.name : '画像';
      btn.appendChild(img);

      const dot = document.createElement('span');
      dot.className = `status-dot status-${session.status}`;
      btn.appendChild(dot);

      stripEl.appendChild(btn);
    }
  }
}