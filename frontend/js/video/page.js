/**
 * video.html のエントリーポイント。画面の流れ（仕様 §7）:
 *   1. 動画を選ぶ → 2. 範囲を選ぶ（10 秒まで）→「この範囲で決める」で POST /video-jobs
 *   3. 署名付き URL へ PUT（進捗バー）と、4. 開始位置のフレームのプレビューを並行して進める
 *      （POST /video-jobs/{id}/preview-frame で登録 → タップのたびに POST /video-jobs/{id}/preview。本番と同じ SAM2.1 tiny）
 *   5. 背景色を選んで送信（POST /video-jobs/{id}/start）→ 6. 1 秒ごとに GET して結果を出す
 *
 * 送信ボタンが押せるのは「アップロード完了」「点が 1 つ以上」「今の点のプレビューが出ていて対象が見つかっている
 * （X-Object-Present: true）」「まだ送っていない」のときだけ。
 * ジョブを作った後は範囲を変えさせない（ジョブの範囲はサーバーに登録済みのため）。
 * 残り回数は POST /video-jobs と GET の応答の remaining_today で更新する（それより前は「1日3回まで」）。
 * 0 になったら動画の選択で止める。
 */
import { createVideoJob, startVideoJob } from '../api.js';
import { showToast } from '../toast.js';
import { clampRange, captureFrame, formatMs, MAX_CLIP_MS } from './range.js';
import { createFramePreview } from './preview.js';
import { putWithProgress } from './upload.js';
import { pollJob } from './job.js';
import {
  apiErrorMessage,
  failureMessage,
  phaseMessage,
  refundMessage,
  remainingMessage,
  SERVER_RESTARTED_MESSAGE,
} from './messages.js';

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
  reselectBtn: $('reselect-btn'),
  decideBtn: $('decide-btn'),
  stepUpload: $('step-upload'),
  uploadProgress: $('upload-progress'),
  uploadStatus: $('upload-status'),
  uploadRetryBtn: $('upload-retry-btn'),
  stepPreview: $('step-preview'),
  previewRetryBtn: $('preview-retry-btn'),
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
  };
}

let state = initialState();
let remainingToday = null; // まだ分からない間は null
let generation = 0; // やり直したら進める。前の動画の非同期処理の結果を捨てるため

// createFramePreview は作る途中で onChange（updateSendButton）を呼ぶので、先に null で宣言しておく
let preview = null;
preview = createFramePreview({
  canvasEl: $('frame-canvas'),
  modeToggleBtn: $('mode-toggle'),
  undoBtn: $('undo-btn'),
  redoBtn: $('redo-btn'),
  statusEl: $('preview-status'),
  warningEl: $('preview-warning'),
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
  updateSelectAvailability();
}

/** 残り 0 なら選ばせない。送信後は結果が出るまで別の動画を選ばせない（進行中のジョブを見失うため） */
function updateSelectAvailability() {
  const exhausted = remainingToday === 0;
  el.quotaOut.hidden = !exhausted;
  el.selectBtn.disabled = exhausted || state.creating || state.sending || (state.sent && !state.finished);
}

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
      setRange(0, MAX_CLIP_MS);
      el.stepRange.hidden = false;
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

el.reselectBtn.addEventListener('click', () => {
  resetAll();
  el.fileInput.click();
});

el.decideBtn.addEventListener('click', () => createJob());

async function createJob() {
  if (state.job || state.creating || state.endMs <= state.startMs) return;
  const gen = generation;
  state.creating = true;
  updateRangeControls();
  updateSelectAvailability();
  try {
    const job = await createVideoJob({
      sizeBytes: state.file.size,
      contentType: state.contentType,
      startMs: state.startMs,
      endMs: state.endMs,
    });
    if (gen !== generation) return;
    state.job = { jobId: job.jobId, uploadUrl: job.uploadUrl };
    setRemaining(job.remainingToday);
    el.stepUpload.hidden = false;
    el.stepPreview.hidden = false;
    el.stepSend.hidden = false;
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
  el.uploadStatus.textContent = 'アップロード中 0%';
  try {
    // Content-Type は POST /video-jobs で申告したもの（state.contentType）と同じでなければ署名が合わない
    state.upload = putWithProgress(state.job.uploadUrl, state.file, state.contentType, (ratio) => {
      if (gen !== generation) return;
      el.uploadProgress.value = ratio;
      el.uploadStatus.textContent = `アップロード中 ${Math.floor(ratio * 100)}%`;
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
    const ok = await preview.load(state.job.jobId, state.frameBlob);
    if (gen !== generation) return;
    if (!ok) el.previewRetryBtn.hidden = false;
  } catch (err) {
    if (gen !== generation) return;
    showToast({ type: 'danger', message: `開始位置のフレームを取り出せませんでした: ${err.message}` });
    el.previewRetryBtn.hidden = false;
  }
}

el.previewRetryBtn.addEventListener('click', () => startPreview());

// ---------- 5. 背景色を選んで送信 ----------

function selectedBackground() {
  const checked = el.backgroundField.querySelector('input[name="background"]:checked');
  return checked ? checked.value : 'green';
}

function updateSendButton() {
  if (!preview) return;
  const ready = state.job !== null && state.uploadDone && preview.canSend();
  el.sendBtn.disabled = !ready || state.sending || state.sent;
  el.backgroundField.disabled = state.sending || state.sent;

  let hint = '';
  if (state.sent || state.sending) hint = '';
  else if (!state.uploadDone) hint = 'アップロードが終わると送信できます';
  else if (preview.pointCount() === 0) hint = '残したい所を 1 か所以上タップすると送信できます';
  else if (preview.objectMissing()) hint = '対象が見つからないため送信できません。点を打ち直してください';
  else if (!preview.canSend()) hint = 'プレビューが出ると送信できます';
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
      points: preview.videoPoints(),
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
  el.stepResult.hidden = false;
  el.jobFailure.hidden = true;
  if (phase) showPhase(phase, null);
  else el.jobStatus.textContent = '送信の結果を確かめています…';
  if (state.poller) state.poller.stop();
  state.poller = pollJob(state.job.jobId, (job) => {
    if (gen === generation) handleJobUpdate(job);
  });
  el.stepResult.scrollIntoView({ behavior: 'smooth', block: 'start' });
}

/** GET で見たらまだ created（開始の要求がサーバーに届いていなかった）。もう一度送れるように戻す */
function handleNotStarted() {
  if (state.poller) state.poller.stop();
  state.poller = null;
  state.sent = false;
  preview.unlock();
  el.stepResult.hidden = true;
  showToast({ type: 'warning', message: '送信できていませんでした。もう一度「送信して切り抜く」を押してください' });
  updateSendButton();
}

function showPhase(phase, progress) {
  el.jobStatus.textContent = phaseMessage(phase, progress);
  el.jobProgress.hidden = phase !== 'tracking';
  if (phase === 'tracking') el.jobProgress.value = progress ?? 0;
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
    el.resultVideo.src = job.downloadUrl;
    el.downloadLink.href = job.downloadUrl;
    el.restartBtn.hidden = false;
  } else if (job.phase === 'failed') {
    el.jobFailure.hidden = false;
    el.jobFailure.textContent = `${failureMessage(job.failureCode)}（${refundMessage(job.quotaRefunded)}）`;
    el.restartBtn.hidden = false;
  }
}

function showLost() {
  state.finished = true;
  updateSelectAvailability();
  el.stepResult.hidden = false;
  el.jobStatus.textContent = '';
  el.jobProgress.hidden = true;
  el.jobFailure.hidden = false;
  el.jobFailure.textContent = SERVER_RESTARTED_MESSAGE;
  el.restartBtn.hidden = false;
}

el.restartBtn.addEventListener('click', () => {
  resetAll();
  el.stepSelect.scrollIntoView({ behavior: 'smooth', block: 'start' });
});

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
  el.downloadLink.href = '#';
  preview.reset();

  for (const step of [el.stepRange, el.stepUpload, el.stepPreview, el.stepSend, el.stepResult]) {
    step.hidden = true;
  }
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
