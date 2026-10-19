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
 *
 * - 複数枚を選んでも、同時に送るのは MAX_CONCURRENT_UPLOADS 件まで（残りは順番待ち）。
 * - アップロードに失敗したサムネイル（赤いドット）は、クリックすると一覧から消せる。
 * - サムネイルのDOMは localId ごとに使い回し、変更のあった属性だけを更新する
 *   （store が更新されるたびに全部作り直すと、ちらつきや画像の再デコードが起きる）。
 */

const MAX_CONCURRENT_UPLOADS = 3;

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
  const thumbs = new Map(); // localId -> { btn, dot }

  const uploadQueue = [];
  let runningUploads = 0;

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
    const localId = thumb.dataset.localId;
    const session = store.getSession(localId);
    if (!session) return;

    if (session.status === 'error') {
      // 失敗したアップロードは、クリックで一覧から取り除く
      thumbs.get(localId)?.btn.remove();
      thumbs.delete(localId);
      store.removeSession(localId);
      return;
    }
    setActive(localId);
  });

  store.subscribe((state) => render(state));
  render(store.getState());

  function handleFiles(fileList) {
    const files = Array.from(fileList || []).filter((f) => f.type.startsWith('image/'));
    for (const file of files) {
      const localId = createLocalId();
      const thumbUrl = URL.createObjectURL(file);

      store.createSession(localId, { thumbUrl, file });
      enqueueUpload(() =>
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
          }),
      );
    }
  }

  // 同時アップロード数を制限する（大量に選んだときに一斉送信して詰まらせない）
  function enqueueUpload(task) {
    uploadQueue.push(task);
    pumpUploads();
  }

  function pumpUploads() {
    while (runningUploads < MAX_CONCURRENT_UPLOADS && uploadQueue.length > 0) {
      const task = uploadQueue.shift();
      runningUploads += 1;
      task().finally(() => {
        runningUploads -= 1;
        pumpUploads();
      });
    }
  }

  function setActive(localId) {
    activeLocalId = localId;
    render(store.getState());
    onSelect(localId);
  }

  function createThumb(session) {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'strip-thumb';
    btn.dataset.localId = session.localId;

    const img = document.createElement('img');
    img.src = session.thumbUrl;
    img.alt = session.file ? session.file.name : '画像';
    btn.appendChild(img);

    const dot = document.createElement('span');
    btn.appendChild(dot);

    stripEl.appendChild(btn);
    return { btn, dot };
  }

  function render(state) {
    const hasImages = state.sessions.size > 0;
    dropzoneEl.hidden = hasImages;
    stripWrapEl.hidden = !hasImages;

    // ストアから無くなったセッションのサムネイルを取り除く
    for (const [localId, thumb] of thumbs) {
      if (!state.sessions.has(localId)) {
        thumb.btn.remove();
        thumbs.delete(localId);
      }
    }

    for (const session of state.sessions.values()) {
      let thumb = thumbs.get(session.localId);
      if (!thumb) {
        thumb = createThumb(session);
        thumbs.set(session.localId, thumb);
      }

      const name = session.file ? session.file.name : '画像';
      thumb.btn.disabled = session.status === 'uploading';
      thumb.btn.classList.toggle('is-active', session.localId === activeLocalId);
      thumb.btn.title = session.status === 'error' ? `${name}（アップロード失敗・クリックで削除）` : name;
      thumb.dot.className = `status-dot status-${session.status}`;
    }
  }
}