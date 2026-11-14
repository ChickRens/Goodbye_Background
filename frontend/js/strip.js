import { uploadImage } from './api.js?v=1.5.2';
import { createLocalId } from './id.js?v=1.5.2';
import { showToast } from './toast.js?v=1.5.2';

/**
 * アップロードの入口は2つある。
 * - dropzoneEl: 画像が1枚も無い間だけ表示する大きいドロップゾーン
 * - addBtnEl（一覧の左端の「＋ 写真を追加」ボタン）: 画像が1枚でもあれば表示する、
 *   小さいストリップ側の入口
 * どちらも同じ handleFiles() に合流する。ホーム画面(index.html)で選ばれた写真も、
 * app.js がこの関数(initStrip の戻り値)経由で同じところに流し込む。1枚でもセッションができた時点で
 * dropzoneEl を隠し、stripWrapEl（矢印＋サムネイル一覧）を表示する——
 * これにより「最初は大きく、画像が増えたら小さく」という見た目になる。
 *
 * サムネイルをクリックすると、そのセッションが編集画面（editor.js）に
 * 切り替わる（onSelectコールバック経由）。
 *
 * 写真を追加したら、いちばん新しく追加した写真を自動で編集画面に開く（2回目以降の追加でも）。
 * - 複数枚をまとめて選んだときは、その中の最後の1枚。アップロードが終わった時点で開く
 *   （同時に送るので完了順はばらばらだが、開くのは1回だけにして画面が何度も切り替わらないようにする）。
 * - 最後の1枚のアップロードに失敗したら、同じ回に選んだ写真のうち成功した最後の1枚を開く。
 * - 開く前に利用者が自分でサムネイルを選んだら、その選択を優先して自動では開かない。
 * - さらに次の写真が追加されたら、前の回の自動表示は取りやめて新しい回の写真を開く。
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
  const thumbs = new Map(); // localId -> { btn, dot }

  const uploadQueue = [];
  let runningUploads = 0;

  // 自動で開く対象の「回」（直近の handleFiles 1回分）。null なら自動では開かない。
  // { order: localId[]（選んだ順）, pending: Set（アップロード中）, succeeded: Set（成功） }
  let autoOpenBatch = null;

  dropzoneEl.addEventListener('click', () => fileInputEl.click());
  addBtnEl.addEventListener('click', () => fileInputEl.click());

  fileInputEl.addEventListener('change', (evt) => {
    handleFiles(evt.target.files);
    fileInputEl.value = ''; // 同じファイルを連続選択できるようにリセット
  });

  // 大きいドロップゾーン・ストリップどちらにもドラッグ＆ドロップ対応（PC向け）
  for (const el of [dropzoneEl, stripEl, addBtnEl]) {
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
  // （CSS側で `pointer: coarse` のときは非表示にしている）。
  // サムネイルが帯からあふれているときだけ表示する（updateArrows）。
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
      // 失敗したアップロードは、クリックで一覧から取り除く（サムネイルのDOMは render が片付ける）
      store.removeSession(localId);
      return;
    }
    autoOpenBatch = null; // 利用者が選んだ写真を優先し、アップロード完了で勝手に切り替えない
    setActive(localId);
  });

  store.subscribe((state) => render(state));
  render(store.getState());

  // 画面幅が変わったとき（ウィンドウのリサイズ・帯が表示に切り替わったとき）にも、あふれているかを判定し直す
  new ResizeObserver(() => updateArrows()).observe(stripWrapEl);

  // ホーム画面から渡された写真を app.js が流し込めるよう、入口の handleFiles を外に出す。
  // select はリロード後に、前回開いていた写真を開き直すのに使う
  return { handleFiles, select: setActive };

  function handleFiles(fileList) {
    const files = Array.from(fileList || []).filter((f) => f.type.startsWith('image/'));
    if (files.length === 0) return;

    const batch = { order: [], pending: new Set(), succeeded: new Set() };
    autoOpenBatch = batch;

    for (const file of files) {
      const localId = createLocalId();
      batch.order.push(localId);
      batch.pending.add(localId);
      const thumbUrl = URL.createObjectURL(file);

      store.createSession(localId, { thumbUrl, file });
      enqueueUpload(() =>
        uploadImage(file)
          .then(({ sessionId, imageId }) => {
            store.updateSession(localId, { sessionId, imageId, status: 'ready', touchedAt: Date.now() });
            batch.pending.delete(localId);
            batch.succeeded.add(localId);
            maybeAutoOpen(batch);
          })
          .catch((err) => {
            store.updateSession(localId, { status: 'error', error: err.message });
            batch.pending.delete(localId);
            maybeAutoOpen(batch);
            showToast({
              type: 'danger',
              message: `${file.name} のアップロードに失敗しました: ${err.message}`,
              code: err.code,
            });
          }),
      );
    }
  }

  // その回の最後の1枚が開けるようになったら開く（説明はファイル冒頭のコメント）
  function maybeAutoOpen(batch) {
    if (autoOpenBatch !== batch) return; // もっと新しい回がある / 利用者が自分で選んだ

    const last = batch.order[batch.order.length - 1];
    if (batch.succeeded.has(last)) {
      autoOpenBatch = null;
      setActive(last);
      return;
    }
    if (batch.pending.has(last)) return; // 最後の1枚がまだアップロード中

    // 最後の1枚は失敗した。残りが全部終わるのを待ってから、成功した最後の1枚を開く
    if (batch.pending.size > 0) return;
    autoOpenBatch = null;
    const fallback = [...batch.order].reverse().find((id) => batch.succeeded.has(id) && store.getSession(id));
    if (fallback) setActive(fallback);
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

  // 「今どの画像を開いているか」は store.activeLocalId が唯一の持ち主。
  // setActive が subscribe 経由で render を呼ぶので、ここで直接 render しない。
  function setActive(localId) {
    store.setActive(localId);
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
      thumb.btn.classList.toggle('is-active', session.localId === state.activeLocalId);
      thumb.btn.title =
        session.status === 'error' ? `${name}（アップロード失敗・クリックで削除）`
        : session.status === 'expired' ? `${name}（時間が経ったため、開くと送り直します）`
        : name;
      thumb.dot.className = `status-dot status-${session.status}`;
    }

    updateArrows();
  }

  /**
   * サムネイルが帯の幅に収まらないときだけ矢印を出す。
   * 矢印を出すと帯が狭くなるので、「矢印が無い状態で」あふれるかを測る
   * （矢印がある状態で測ると、矢印を消せば収まる場合でも出しっぱなしになる）。
   * 一度隠して測り、同じ処理の中で表示を戻すので、画面がちらつくことはない。
   */
  function updateArrows() {
    prevBtnEl.hidden = true;
    nextBtnEl.hidden = true;
    const overflowing = stripEl.scrollWidth > stripEl.clientWidth + 1;
    prevBtnEl.hidden = !overflowing;
    nextBtnEl.hidden = !overflowing;
  }
}