/**
 * ホーム画面(index.html)の選択エリア。写真と動画の両方を受け付け、種類で行き先を分ける。
 *   - 写真(1枚以上) → handoff.js に保存して app.html へ（js/app.js が取り出してアップロードを始める）
 *   - 動画(1本)     → handoff.js に保存して video.html へ（js/video/page.js が取り出して範囲の選択から始める）
 * 写真と動画を一緒に選んだとき・動画を2本以上選んだときは、移動せずにエリアの下で選び直してもらう。
 */
import { saveFiles, saveVideo } from './handoff.js?v=1.6.2';
import { trackVisit } from './visit.js?v=1.6.2';

// 流入元（utm 付きの URL）の記録。下の処理が URL を書き換える前に呼ぶ
trackVisit();

const zone = document.getElementById('start-zone');
const input = document.getElementById('start-input');
const errorEl = document.getElementById('start-error');

// iPhone などで file.type が空のことがあるので、拡張子でも見分ける
const VIDEO_EXTENSIONS = /\.(mp4|m4v|mov)$/i;
const isVideo = (f) => f.type.startsWith('video/') || VIDEO_EXTENSIONS.test(f.name || '');
const isImage = (f) => f.type.startsWith('image/');

function showError(message) {
  errorEl.textContent = message;
  errorEl.hidden = false;
}

async function start(fileList) {
  const all = Array.from(fileList || []);
  const videos = all.filter(isVideo);
  const images = all.filter((f) => !isVideo(f) && isImage(f));

  if (videos.length > 0 && images.length > 0) {
    showError('写真と動画は一緒に選べません。どちらか一方を選んでください。');
    return;
  }
  if (videos.length > 1) {
    showError('動画は1本ずつ選んでください。');
    return;
  }
  if (videos.length === 0 && images.length === 0) {
    showError('写真（JPEG・PNG・WebP など）か、動画（MP4・MOV）を選んでください。');
    return;
  }

  errorEl.hidden = true;
  zone.classList.add('is-busy');
  // 受け渡しに失敗したら、移動先の画面でその旨を伝えて選び直してもらう
  if (videos.length === 1) {
    const saved = await saveVideo(videos[0]);
    location.href = saved ? 'video.html' : 'video.html?handoff=failed';
  } else {
    const saved = await saveFiles(images);
    location.href = saved ? 'app.html' : 'app.html?handoff=failed';
  }
}

input.addEventListener('change', () => {
  start(input.files);
  input.value = ''; // 戻るボタンで戻ってきたとき、同じものをもう一度選べるようにする
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

// ---------- 切り抜きの例（動画） ----------
// 例の動画(assets/example-video-*.mp4)は git 管理外。置かれていない・再生できないときは、
// 壊れた枠を見せないよう、動画の例をまるごと隠す（写真の例だけが残る）
const videoExample = document.getElementById('example-video');
if (videoExample) {
  const videos = videoExample.querySelectorAll('video');
  const hide = () => { videoExample.hidden = true; };
  for (const v of videos) {
    v.addEventListener('error', hide);
    for (const s of v.querySelectorAll('source')) s.addEventListener('error', hide);
    // このスクリプトが動く前に読み込みが失敗していることもある（ファイルが無くてすぐ 404 が返るとき）
    if (v.error || v.networkState === HTMLMediaElement.NETWORK_NO_SOURCE) hide();
  }
  // 「動きを減らす」設定では自動再生しない（最初のコマを止めて見せ、押せば再生できるようにする）
  if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
    for (const v of videos) {
      v.removeAttribute('autoplay');
      v.pause();
      v.controls = true;
    }
  } else if (videos.length === 2) {
    // 切り抜き前と後を同じ瞬間で見比べられるよう、後の動画を前の動画の再生位置に合わせ続ける
    // （別々にループさせると、読み込みの差やループの継ぎ目で少しずつずれていく）
    const [before, after] = videos;
    before.addEventListener('timeupdate', () => {
      if (Math.abs(after.currentTime - before.currentTime) > 0.15) after.currentTime = before.currentTime;
    });
  }
}
