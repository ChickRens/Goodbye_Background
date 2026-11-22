/**
 * video.html のエントリーポイント。画面の流れ（仕様 §7）:
 *   1. 動画を選ぶ → 2. 範囲を選ぶ（10 秒まで）→「この範囲で決める」で POST /video-jobs
 *   3. 署名付き URL へ PUT（進捗バー）と、4. 開始位置のフレームのプレビューを並行して進める
 *      （POST /video-jobs/{id}/preview-frame?time_ms= で登録 → タップのたびに POST /video-jobs/{id}/preview。本番と同じ SAM2.1 small）
 *      2 枚目以降のフレーム（最大 5 枚）も同じように点を打てる（frame_set.js）
 *   5. 背景色を選んで送信（POST /video-jobs/{id}/start）→ 6. 1 秒ごとに GET して結果を出す
 *
 * 送信ボタンが押せるのは「アップロード完了」「点が 1 つ以上」「今の点のプレビューが出ていて対象が見つかっている
 * （X-Object-Present: true）」「まだ送っていない」のときだけ。
 * ジョブを作った後は範囲を変えさせない（ジョブの範囲はサーバーに登録済みのため）。
 * 残り回数は POST /video-jobs と GET の応答の remaining_today で更新する（それより前は「1日3回まで」）。
 * 0 になったら動画の選択で止める。
 *
 * 見た目は「今の手順だけを出す」形（renderView）。上の進み具合(#stepper)で今どこにいるかを見せ、
 * 終わった手順は表示しない。PC は左にステージ・右に操作パネル、スマホは縦に並べる（css/video.css）。
 * 「切り抜き」（残したい所をタップ）と「背景色」は別の手順で、「次へ」「切り抜きに戻る」で行き来する（送信するまで）。
 * 切り抜きの操作（残す／消す・元に戻す・やり直す・次へ）は、写真のアプリと同じく下のツールバー(.vtools)にまとめる。
 * アップロードは手順にせず裏で進め、進み具合は送信に要る「背景色」の手順で初めて出す。
 * ホーム画面で動画が選ばれていれば、起動時に受け取って範囲の選択から始める（js/handoff.js）。
 */
import { createVideoJob, startVideoJob } from '../api.js?v=1.6.2';
import { takeVideo } from '../handoff.js?v=1.6.2';
import { showToast } from '../toast.js?v=1.6.2';
import { clampRange, captureFrame, formatMs, MAX_CLIP_MS } from './range.js?v=1.6.2';
import { createFrameSet } from './frame_set.js?v=1.6.2';
import { formatFrameTime, MAX_FRAMES } from './frames.js?v=1.6.2';
import { putWithProgress } from './upload.js?v=1.6.2';
import { toJobTime, trimForUpload } from './trim.js?v=1.6.2';
import { pollJob } from './job.js?v=1.6.2';
import {
  apiErrorMessage,
  failureMessage,
  phaseMessage,
  refundMessage,
  remainingMessage,
  SERVER_RESTARTED_MESSAGE,
} from './messages.js?v=1.6.2';
import { trackVisit } from '../visit.js?v=1.6.2';

// 流入元（utm 付きの URL）の記録。下の処理が URL を書き換える前に呼ぶ
trackVisit();

/** アップロードの上限（サーバーの MAX_INPUT_BYTES と同じ 200MB） */
const MAX_INPUT_BYTES = 209_715_200;

/** サーバーが受け付ける Content-Type。PUT の署名にも入るので、決めた文字列を両方のリクエストで使う */
const CONTENT_TYPE_BY_EXTENSION = {
  mp4: 'video/mp4',
  m4v: 'video/mp4',
  mov: 'video/quicktime',
};
const SUPPORTED_CONTENT_TYPES = new Set(['video/mp4', 'video/quicktime']);

const $ = (id) => document.getElementById(id);

const el = {
  remaining: $('remaining'),
  stepSelect: $('step-select'),
  fileInput: $('video-file'),
  selectBtn: $('select-btn'),
  quotaOut: $('quota-out'),
  stepRange: $('step-range'),
  video: $('source-video'),
  rangeStart: $('range-start'),
  rangeEnd: $('range-end'),
  rangeStartLabel: $('range-start-label'),
  rangeEndLabel: $('range-end-label'),
  rangeLength: $('range-length'),
  rangeBarFill: $('range-bar-fill'),
  rangeTotal: $('range-total'),
  stepper: $('stepper'),
  vpage: $('vpage'),
  nextBtn: $('next-btn'),
  nextHint: $('next-hint'),
  backBtn: $('back-btn'),
  reselectBtn2: $('reselect-btn-2'),
  jobWait: $('job-wait'),
  jobMeterLabel: $('job-meter-label'),
  resultTitle: $('result-title'),
  reselectBtn: $('reselect-btn'),
  decideBtn: $('decide-btn'),
  stepUpload: $('step-upload'),
  uploadProgress: $('upload-progress'),
  uploadStatus: $('upload-status'),
  uploadRetryBtn: $('upload-retry-btn'),
  stepPreview: $('step-preview'),
  previewRetryBtn: $('preview-retry-btn'),
  addFrameBtn: $('add-frame-btn'),
  addFrameText: document.querySelector('#add-frame-btn .frame-add-text'),
  frameCount: $('frame-count'),
  picker: $('frame-picker'),
  pickerVideo: $('picker-video'),
  pickerTime: $('picker-time'),
  pickerLabel: $('picker-label'),
  pickerDecide: $('picker-decide'),
  pickerCancel: $('picker-cancel'),
  stepSend: $('step-send'),
  backgroundField: $('background-field'),
  sendBtn: $('send-btn'),
  sendHint: $('send-hint'),
  stepResult: $('step-result'),
  jobStatus: $('job-status'),
  jobProgress: $('job-progress'),
  jobFailure: $('job-failure'),
  jobDone: $('job-done'),
  resultVideo: $('result-video'),
  downloadLink: $('download-link'),
  restartBtn: $('restart-btn'),
};

/** 1 本の動画についての状態。「最初からやり直す」で作り直す（remaining だけは引き継ぐ） */
function initialState() {
  return {
    file: null,
    contentType: null,
    videoUrl: null,
    durationMs: 0,
    startMs: 0,
    endMs: 0,
    job: null, // { jobId, uploadUrl }
    creating: false,
    uploadDone: false,
    frameBlob: null,
    sending: false,
    sent: false,
    finished: false, // done / failed / 404 になった
    poller: null,
    upload: null, // putWithProgress の { promise, abort }
    uploadBlob: null, // 送るもの（選んだ範囲だけ切り出した動画。切り出せなければ元のファイル）
    offsetMs: 0, // 送る動画の時刻 0 が元の動画の何ミリ秒か（切り出していなければ 0。trim.js）
    queuedSince: null, // 準備中（GPU の起動待ち）になった時刻（Date.now）。経過時間の表示に使う
  };
}

let state = initialState();
let remainingToday = null; // まだ分からない間は null
let generation = 0; // やり直したら進める。前の動画の非同期処理の結果を捨てるため

// 点を打つフレームの組（1 枚目 = 開始位置、最大 5 枚）。作る途中で onChange（updateSendButton）を呼ぶので、先に null で宣言しておく
let preview = null;
preview = createFrameSet({
  tabsEl: $('frame-tabs'),
  panelsEl: $('frame-panels'),
  panelTemplate: $('frame-panel-template'),
  modeSwitchEl: $('mode-switch'),
  undoBtn: $('undo-btn'),
  redoBtn: $('redo-btn'),
  onChange: updateSendButton,
  onError: (err, what) => {
    showToast({ type: 'danger', message: `${what}に失敗しました: ${apiErrorMessage(err)}`, code: err.code });
    if (!state.sent && !state.sending) el.previewRetryBtn.hidden = false;
  },
});

// ---------- 残り回数 ----------

function setRemaining(value) {
  if (typeof value !== 'number') return;
  remainingToday = value;
  el.remaining.textContent = remainingMessage(value);
  // ジョブを作る前に使い切ったら、範囲の欄は閉じる（決めても作れないため）。作った後なら送信まで続けられる
  updateSelectAvailability();
}

/** 残り 0 なら選ばせない。送信後は結果が出るまで別の動画を選ばせない（進行中のジョブを見失うため） */
function updateSelectAvailability() {
  const exhausted = remainingToday === 0;
  el.quotaOut.hidden = !exhausted;
  el.selectBtn.disabled = exhausted || state.creating || state.sending || (state.sent && !state.finished);
  renderView();
}

// ---------- 手順の表示（今の手順だけを出す） ----------

const STEP_ORDER = ['select', 'range', 'tap', 'send', 'result'];
// 「切り抜き」と「背景色」のどちらを見ているか（ジョブを作ってから送信するまでの間だけ使う）
let jobView = 'tap';
let shownView = 'select';

/** 今出す手順 */
function currentView() {
  if (state.sent) return 'result';
  if (state.job) return jobView;
  // 今日の回数を使い切ったら、「動画を選ぶ」（使い切った旨の案内がある）に戻す
  if (remainingToday === 0) return 'select';
  if (state.durationMs > 0) return 'range';
  return 'select';
}

function renderView() {
  if (!el.stepper || !preview) return;
  const view = currentView();
  const index = STEP_ORDER.indexOf(view);
  el.vpage.dataset.view = view;

  // ステージとパネルの中身は、data-views に今の手順が書いてあるものだけ出す
  for (const node of el.vpage.querySelectorAll('[data-views]')) {
    node.hidden = !node.dataset.views.split(' ').includes(view);
  }

  const finishedOk = state.finished && !el.jobDone.hidden;
  for (const li of el.stepper.querySelectorAll('[data-step]')) {
    const i = STEP_ORDER.indexOf(li.dataset.step);
    const done = i < index || (li.dataset.step === 'result' && finishedOk);
    li.classList.toggle('is-done', done);
    li.classList.toggle('is-current', i === index && !done);
    if (i === index && !done) li.setAttribute('aria-current', 'step');
    else li.removeAttribute('aria-current');
  }

  // 手順が変わったら、スマホでは先頭（進み具合）から見せる（PC は1画面に収まっているので動かない）
  if (view !== shownView) {
    shownView = view;
    if (window.scrollY > 0) {
      const smooth = !window.matchMedia('(prefers-reduced-motion: reduce)').matches;
      window.scrollTo({ top: 0, behavior: smooth ? 'smooth' : 'auto' });
    }
  }
}

function chooseAnotherVideo() {
  resetAll();
  el.fileInput.click();
}
el.reselectBtn2.addEventListener('click', chooseAnotherVideo);

// 切り抜き → 背景色。送れる点の指定ができてから進める
el.nextBtn.addEventListener('click', () => {
  if (el.nextBtn.disabled) return;
  jobView = 'send';
  renderView();
});
// 背景色 → 切り抜き（送信するまでは戻って点を直せる）
el.backBtn.addEventListener('click', () => {
  if (state.sending || state.sent) return;
  jobView = 'tap';
  renderView();
});

// ---------- 1. 動画を選ぶ ----------

el.selectBtn.addEventListener('click', () => el.fileInput.click());
el.fileInput.addEventListener('change', () => {
  const file = el.fileInput.files && el.fileInput.files[0];
  el.fileInput.value = ''; // 同じファイルをもう一度選んでも change が来るように
  if (file) openVideo(file);
});

/** file.type が対応形式ならそれ、無ければ拡張子から決める。どちらでもなければ null */
function detectContentType(file) {
  if (SUPPORTED_CONTENT_TYPES.has(file.type)) return file.type;
  const match = /\.([^.]+)$/.exec(file.name || '');
  const ext = match ? match[1].toLowerCase() : '';
  return CONTENT_TYPE_BY_EXTENSION[ext] || null;
}

function openVideo(file) {
  if (remainingToday === 0) return;
  if (file.type.startsWith('image/')) {
    showToast({ type: 'info', message: '写真は、上の「写真」の画面で切り抜けます' });
    return;
  }
  const contentType = detectContentType(file);
  if (!contentType) {
    showToast({ type: 'warning', message: 'この形式の動画には対応していません（MP4 / MOV のみ）' });
    return;
  }
  if (file.size > MAX_INPUT_BYTES) {
    showToast({ type: 'warning', message: '動画が大きすぎます（200MB まで）' });
    return;
  }

  resetAll();
  const gen = generation;
  state.file = file;
  state.contentType = contentType;
  state.videoUrl = URL.createObjectURL(file);

  el.video.addEventListener(
    'loadedmetadata',
    () => {
      if (gen !== generation) return;
      const durationMs = Math.floor(el.video.duration * 1000);
      if (!Number.isFinite(durationMs) || durationMs <= 0) {
        showToast({ type: 'danger', message: '動画の長さを読み取れませんでした。別の動画で試してください' });
        resetAll();
        return;
      }
      state.durationMs = durationMs;
      el.rangeStart.max = String(durationMs);
      el.rangeEnd.max = String(durationMs);
      el.rangeTotal.textContent = formatMs(durationMs);
      setRange(0, MAX_CLIP_MS);
      renderView();
    },
    { once: true },
  );
  el.video.addEventListener(
    'error',
    () => {
      if (gen !== generation || state.job) return;
      showToast({ type: 'danger', message: 'この動画はこのブラウザで再生できません。別の動画で試してください' });
      resetAll();
    },
    { once: true },
  );
  el.video.src = state.videoUrl;
  el.video.load();
}

// ---------- 2. 範囲を選ぶ ----------

function setRange(startMs, endMs) {
  const range = clampRange(startMs, endMs, state.durationMs);
  state.startMs = range.startMs;
  state.endMs = range.endMs;
  el.rangeStart.value = String(range.startMs);
  el.rangeEnd.value = String(range.endMs);
  el.rangeStartLabel.textContent = formatMs(range.startMs);
  el.rangeEndLabel.textContent = formatMs(range.endMs);
  el.rangeLength.textContent = `長さ ${((range.endMs - range.startMs) / 1000).toFixed(1)} 秒（10 秒まで）`;
  // 動画全体のうち選んでいる範囲に色を付ける
  if (state.durationMs > 0) {
    el.rangeBarFill.style.left = `${(range.startMs / state.durationMs) * 100}%`;
    el.rangeBarFill.style.width = `${((range.endMs - range.startMs) / state.durationMs) * 100}%`;
  }
  updateRangeControls();
}

function seekVideo(ms) {
  el.video.pause();
  el.video.currentTime = ms / 1000;
}

el.rangeStart.addEventListener('input', () => {
  const startMs = Number(el.rangeStart.value);
  // 開始が終了を追い越したら、終了を開始 + 10 秒まで押し出す
  const endMs = state.endMs <= startMs ? startMs + MAX_CLIP_MS : state.endMs;
  setRange(startMs, endMs);
  seekVideo(state.startMs);
});

el.rangeEnd.addEventListener('input', () => {
  setRange(state.startMs, Number(el.rangeEnd.value));
  seekVideo(state.endMs);
});

function updateRangeControls() {
  const locked = state.job !== null || state.creating;
  el.rangeStart.disabled = locked;
  el.rangeEnd.disabled = locked;
  el.reselectBtn.disabled = locked;
  el.decideBtn.disabled = locked || state.endMs <= state.startMs;
}

el.reselectBtn.addEventListener('click', chooseAnotherVideo);

el.decideBtn.addEventListener('click', () => createJob());

async function createJob() {
  if (state.job || state.creating || state.endMs <= state.startMs) return;
  const gen = generation;
  state.creating = true;
  updateRangeControls();
  updateSelectAvailability();
  try {
    // 選んだ範囲だけを切り出して送る（再エンコードしない）。切り出せなければ元のファイルを丸ごと送る
    const trimmed = await trimForUpload(state.file, state.startMs, state.endMs);
    if (gen !== generation) return;
    state.uploadBlob = trimmed?.blob ?? state.file;
    state.offsetMs = trimmed?.offsetMs ?? 0;
    const job = await createVideoJob({
      sizeBytes: state.uploadBlob.size,
      contentType: state.contentType,
      startMs: toJobTime(state.startMs, state.offsetMs),
      endMs: toJobTime(state.endMs, state.offsetMs),
    });
    if (gen !== generation) return;
    state.job = { jobId: job.jobId, uploadUrl: job.uploadUrl };
    setRemaining(job.remainingToday);
    jobView = 'tap';
    // アップロードとプレビューは並行して進める
    startUpload();
    startPreview();
  } catch (err) {
    if (gen !== generation) return;
    if (err.code === 'QUOTA_EXCEEDED') setRemaining(0);
    showToast({ type: 'danger', message: apiErrorMessage(err), code: err.code });
  } finally {
    if (gen === generation) {
      state.creating = false;
      updateRangeControls();
      updateSelectAvailability();
      updateSendButton();
    }
  }
}

// ---------- 3. アップロード ----------

async function startUpload() {
  const gen = generation;
  el.uploadRetryBtn.hidden = true;
  el.uploadProgress.value = 0;
  // 送る大きさも出す（選んだ範囲だけ切り出せたかが分かるように）
  const sizeLabel = `${(state.uploadBlob.size / 1_000_000).toFixed(1)} MB`;
  el.uploadStatus.textContent = `アップロード中 0%（${sizeLabel}）`;
  try {
    // Content-Type は POST /video-jobs で申告したもの（state.contentType）と同じでなければ署名が合わない
    state.upload = putWithProgress(state.job.uploadUrl, state.uploadBlob, state.contentType, (ratio) => {
      if (gen !== generation) return;
      el.uploadProgress.value = ratio;
      el.uploadStatus.textContent = `アップロード中 ${Math.floor(ratio * 100)}%（${sizeLabel}）`;
    });
    await state.upload.promise;
    if (gen !== generation) return;
    state.uploadDone = true;
    el.uploadStatus.textContent = 'アップロード完了';
  } catch (err) {
    if (gen !== generation) return;
    el.uploadStatus.textContent = err.message;
    el.uploadRetryBtn.hidden = false;
  } finally {
    if (gen === generation) {
      state.upload = null;
      updateSendButton();
    }
  }
}

el.uploadRetryBtn.addEventListener('click', () => startUpload());

// ---------- 4. 開始位置のフレームでプレビュー ----------

async function startPreview() {
  const gen = generation;
  el.previewRetryBtn.hidden = true;
  try {
    if (!state.frameBlob) {
      state.frameBlob = await captureFrame(el.video, state.startMs);
      if (gen !== generation) return;
    }
    // 1 枚目がもうあれば、表示中のフレームの登録をやり直す（「プレビューをやり直す」）
    const ok = preview.hasFrames()
      ? await preview.retryActive()
      : await preview.start(state.job.jobId, state.startMs, state.endMs, state.frameBlob, state.offsetMs);
    if (gen !== generation) return;
    if (!ok) el.previewRetryBtn.hidden = false;
  } catch (err) {
    if (gen !== generation) return;
    showToast({ type: 'danger', message: `開始位置のフレームを取り出せませんでした: ${err.message}` });
    el.previewRetryBtn.hidden = false;
  }
}

el.previewRetryBtn.addEventListener('click', () => startPreview());

// ---------- 4'. 2 枚目以降のフレームを足す ----------

function pickerOpen() {
  return !el.picker.hidden;
}

function setPickerTime(ms) {
  el.pickerTime.value = String(ms);
  el.pickerLabel.textContent = `${formatFrameTime(ms, state.startMs)} のフレーム`;
  el.pickerVideo.currentTime = ms / 1000;
}

el.addFrameBtn.addEventListener('click', () => {
  if (!state.job || pickerOpen()) return;
  if (!el.pickerVideo.getAttribute('src')) el.pickerVideo.src = state.videoUrl;
  el.pickerTime.min = String(state.startMs);
  el.pickerTime.max = String(state.endMs - 1);
  el.picker.hidden = false;
  setPickerTime(state.startMs);
  updateSendButton();
});

el.pickerTime.addEventListener('input', () => setPickerTime(Number(el.pickerTime.value)));

el.pickerCancel.addEventListener('click', () => {
  el.picker.hidden = true;
  updateSendButton();
});

el.pickerDecide.addEventListener('click', async () => {
  const gen = generation;
  const timeMs = Math.round(Number(el.pickerTime.value));
  if (!preview.canAdd(timeMs)) {
    showToast({ type: 'warning', message: '点を打ったフレームに近すぎます（0.05 秒以上離してください）' });
    return;
  }
  el.pickerDecide.disabled = true;
  try {
    const blob = await captureFrame(el.pickerVideo, timeMs);
    if (gen !== generation) return;
    el.picker.hidden = true;
    const ok = await preview.add(timeMs, blob);
    if (gen !== generation) return;
    if (ok === false) el.previewRetryBtn.hidden = false;
  } catch (err) {
    if (gen !== generation) return;
    showToast({ type: 'danger', message: `フレームを取り出せませんでした: ${err.message}` });
  } finally {
    el.pickerDecide.disabled = false;
    updateSendButton();
  }
});

// ---------- 5. 背景色を選んで送信 ----------

function selectedBackground() {
  const checked = el.backgroundField.querySelector('input[name="background"]:checked');
  return checked ? checked.value : 'green';
}

function updateSendButton() {
  if (!preview) return;
  // 後のプレビューが成功したら（対象の有無は問わない）、前の失敗で出した「プレビューをやり直す」は引っ込める
  if (preview.activeCanSendOrMissing()) el.previewRetryBtn.hidden = true;
  const ready = state.job !== null && state.uploadDone && preview.canSend();
  el.sendBtn.disabled = !ready || state.sending || state.sent;
  el.backgroundField.disabled = state.sending || state.sent;
  el.addFrameBtn.disabled =
    !state.job || !preview.hasFrames() || preview.isFull() || pickerOpen() || state.sending || state.sent;
  el.addFrameText.textContent = preview.isFull() ? '5 枚まで追加しました' : 'フレームを追加';
  el.frameCount.textContent = preview.hasFrames() ? `${preview.frameCount()} / ${MAX_FRAMES} 枚` : '';

  const problem = preview.firstProblem();
  // 「次へ」は、送れる点の指定ができたら押せる（アップロードの完了は送信の手順で待つ）
  el.nextBtn.disabled = !state.job || !preview.canSend() || pickerOpen() || state.sending || state.sent;
  let nextHint = '';
  if (problem?.kind === 'noPoints') nextHint = `${problem.label} のフレームに、残したい所を 1 か所以上タップすると次へ進めます`;
  else if (problem?.kind === 'missing') nextHint = `${problem.label} のフレームで対象が見つかりません。残したい物の真ん中をタップし直してください`;
  else if (state.job && !preview.canSend()) nextHint = '切り抜きの見本が出ると次へ進めます';
  el.nextHint.textContent = nextHint;

  let hint = '';
  if (state.sent || state.sending) hint = '';
  else if (!state.uploadDone && state.upload) hint = 'アップロードが終わると送信できます（もう少しお待ちください）';
  else if (!state.uploadDone) hint = 'アップロードに失敗しました。上の「アップロードをやり直す」を押してください';
  else if (!preview.canSend()) hint = '「切り抜きに戻る」で、点を打ち直してください';
  else hint = '送信すると今日の回数を 1 回使います';
  el.sendHint.textContent = hint;
  updateSelectAvailability();
}

el.sendBtn.addEventListener('click', () => sendJob());

async function sendJob() {
  if (el.sendBtn.disabled) return;
  const gen = generation;
  state.sending = true;
  preview.lock(); // 送った点と画面の点がずれないよう、応答を待つ間も点を変えさせない
  updateSendButton();
  try {
    const started = await startVideoJob(state.job.jobId, {
      prompts: preview.prompts(),
      background: selectedBackground(),
    });
    if (gen !== generation) return;
    setRemaining(started.remainingToday);
    // phase が failed（dispatch_failed）でも GET で回数が戻ったかを取りに行く
    beginPolling(started.phase);
  } catch (err) {
    if (gen !== generation) return;
    if (err.code === 'VIDEO_JOB_INVALID_STATE' || !err.status) {
      // サーバーでは開始できていて、応答だけが届かなかった（通信の切断・タイムアウト）か、
      // 前の送信がもう通っている。どちらも本当の状態は GET で分かるので、成功したときと同じく見に行く。
      // まだ created なら handleJobUpdate が送信ボタンを戻す。
      beginPolling(null);
    } else if (err.status === 404) {
      // サーバーの再起動でジョブが消えた。やり直すしかない
      state.sent = true;
      showLost();
    } else {
      if (err.code === 'QUOTA_EXCEEDED') setRemaining(0);
      preview.unlock(); // 送れていないので、点を直してもう一度送れる
      showToast({ type: 'danger', message: apiErrorMessage(err), code: err.code });
    }
  } finally {
    if (gen === generation) {
      state.sending = false;
      updateSendButton();
    }
  }
}

// ---------- 6. 結果 ----------

/** 送信済みとして結果の欄を出し、ポーリングを始める。phase が分かっていれば先に出しておく */
function beginPolling(phase) {
  const gen = generation;
  state.sent = true;
  preview.lock();
  el.jobFailure.hidden = true;
  el.jobWait.hidden = false;
  el.resultTitle.textContent = '切り抜いています';
  if (phase) showPhase(phase, null);
  else el.jobStatus.textContent = '送信の結果を確かめています…';
  if (state.poller) state.poller.stop();
  state.poller = pollJob(state.job.jobId, (job) => {
    if (gen === generation) handleJobUpdate(job);
  });
  renderView();
}

/** GET で見たらまだ created（開始の要求がサーバーに届いていなかった）。もう一度送れるように戻す */
function handleNotStarted() {
  if (state.poller) state.poller.stop();
  state.poller = null;
  state.sent = false;
  preview.unlock();
  showToast({ type: 'warning', message: '送信できていませんでした。もう一度「送信して切り抜く」を押してください' });
  updateSendButton();
}

function showPhase(phase, progress) {
  // 準備中になった時刻を覚えておき、経過時間を出す（GPU の起動が遅いとき、何分待っているか分かるように）
  if (phase === 'queued') state.queuedSince ??= Date.now();
  else state.queuedSince = null;
  const queuedMs = state.queuedSince === null ? 0 : Date.now() - state.queuedSince;
  el.jobStatus.textContent = phaseMessage(phase, progress, queuedMs);
  // メーター: 処理の割合が分かる間(tracking)は値を出す。分からない間（準備中・ダウンロード・仕上げ）は
  // value を外して、動き続けるメーター（ブラウザ標準の「進行中」表示）にする
  if (phase === 'tracking') {
    const ratio = Math.max(0, Math.min(1, progress ?? 0));
    el.jobProgress.value = ratio;
    el.jobMeterLabel.textContent = `${Math.floor(ratio * 100)}%`;
  } else {
    el.jobProgress.removeAttribute('value');
    el.jobMeterLabel.textContent = phase === 'finishing' ? '仕上げ中' : '準備中';
  }
}

function handleJobUpdate(job) {
  if (job.phase === 'lost') {
    showLost();
    return;
  }
  setRemaining(job.remainingToday);
  if (job.phase === 'created') {
    handleNotStarted();
    return;
  }
  showPhase(job.phase, job.progress);
  if (job.phase === 'done' || job.phase === 'failed') {
    state.finished = true;
    updateSelectAvailability();
  }
  if (job.phase === 'done') {
    el.jobDone.hidden = false;
    el.jobWait.hidden = true;
    el.resultVideo.hidden = false;
    el.resultTitle.textContent = '完成しました';
    el.jobStatus.textContent = ''; // 見出しの「完成しました」と同じことなので出さない
    el.resultVideo.src = job.downloadUrl;
    el.downloadLink.href = job.downloadUrl;
    el.restartBtn.hidden = false;
  } else if (job.phase === 'failed') {
    el.jobWait.hidden = true;
    el.resultTitle.textContent = 'うまく切り抜けませんでした';
    el.jobFailure.hidden = false;
    el.jobFailure.textContent = `${failureMessage(job.failureCode)}（${refundMessage(job.quotaRefunded)}）`;
    el.restartBtn.hidden = false;
  }
  renderView(); // 結果を出した後に、上の進み具合（完成の ✓）を描き直す
}

function showLost() {
  state.finished = true;
  updateSelectAvailability();
  el.jobWait.hidden = true;
  el.resultTitle.textContent = 'やり直してください';
  el.jobStatus.textContent = '';
  el.jobProgress.hidden = true;
  el.jobFailure.hidden = false;
  el.jobFailure.textContent = SERVER_RESTARTED_MESSAGE;
  el.restartBtn.hidden = false;
  renderView();
}

el.restartBtn.addEventListener('click', () => resetAll());

// ---------- やり直し ----------

/** 1 本の動画についての状態と画面を最初に戻す（残り回数は残す） */
function resetAll() {
  generation += 1;
  if (state.poller) state.poller.stop();
  if (state.upload) state.upload.abort(); // 前の動画のアップロードを止める（結果は generation で捨てる）
  if (state.videoUrl) URL.revokeObjectURL(state.videoUrl);
  state = initialState();

  el.video.removeAttribute('src');
  el.video.load();
  el.resultVideo.removeAttribute('src');
  el.resultVideo.load();
  el.resultVideo.hidden = true;
  el.jobWait.hidden = false;
  // メーターを最初の状態（値なしの動き続ける表示）に戻す（showLost で隠していることがある）
  el.jobProgress.hidden = false;
  el.jobProgress.removeAttribute('value');
  el.jobMeterLabel.textContent = '';
  el.resultTitle.textContent = '切り抜いています';
  jobView = 'tap';
  el.downloadLink.href = '#';
  preview.reset();
  el.picker.hidden = true;
  el.pickerVideo.removeAttribute('src');
  el.pickerVideo.load();

  el.uploadRetryBtn.hidden = true;
  el.previewRetryBtn.hidden = true;
  el.jobFailure.hidden = true;
  el.jobDone.hidden = true;
  el.restartBtn.hidden = true;
  el.jobStatus.textContent = '';
  el.uploadStatus.textContent = '';
  const green = el.backgroundField.querySelector('input[value="green"]');
  if (green) green.checked = true;

  updateRangeControls();
  updateSendButton();
  updateSelectAvailability();
}

el.remaining.textContent = remainingMessage(remainingToday);
resetAll();

// ホーム画面で選ばれた動画があれば、そのまま範囲の選択から始める（js/handoff.js）
takeVideo().then((file) => {
  if (file) openVideo(file);
});
if (new URLSearchParams(location.search).get('handoff') === 'failed') {
  showToast({ type: 'warning', message: 'ホーム画面で選んだ動画を受け取れませんでした。ここでもう一度選んでください。' });
  history.replaceState(null, '', location.pathname);
}
