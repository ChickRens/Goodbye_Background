/**
 * 画面上部からスライドインするトースト通知。
 *
 * type ごとに tokens.css の色変数とアイコンを対応させる。
 * 全種類とも一定時間で自動的に消える。消えるタイミングが分かるよう、
 * カード背景下端にカウントダウンの帯（残り時間に応じて縮む）を表示する。
 * カードにマウス/指が乗っている間は、読む時間を確保するため消えるのを止める。
 *
 * 読み上げ: danger/warning は role="alert"（即時に読み上げ）、
 * info/success は role="status"（読み上げ中の内容を遮らない）。
 */

/**
 * mask-image をインラインスタイルとして直接設定する。
 *
 * CSSカスタムプロパティ(--icon-src)にurl()を入れて外部スタイルシート側の
 * mask-image: var(--icon-src) で使うと、そのURLは「変数を定義した場所」
 * ではなく「varを使っている側のスタイルシート」を基準に解決されてしまう
 * （ブラウザの仕様上の挙動）。ここでは css/style.css からの相対パスとして
 * 解決されてしまい css/assets/... を探しにいくバグの原因になっていた。
 * インラインスタイルとして直接設定すれば、HTMLページ(app.html)を基準に
 * 解決されるので、assets/ を正しく参照できる。
 *
 * パスは先頭に `/` を付けない相対パス（HTMLページ基準）。ルート絶対パスにすると、
 * サイトをサブパス配信したときに壊れる。
 */
function applyMaskIcon(el, path) {
  const url = `url("${path}")`;
  el.style.setProperty('-webkit-mask-image', url);
  el.style.setProperty('mask-image', url);
}

const TYPE_CONFIG = {
  info: { colorVar: '--info', icon: 'assets/info.svg', role: 'status' },
  warning: { colorVar: '--warning', icon: 'assets/warning.svg', role: 'alert' },
  danger: { colorVar: '--danger', icon: 'assets/danger.svg', role: 'alert' },
  success: { colorVar: '--success', icon: 'assets/success.svg', role: 'status' },
};

const CLOSE_ICON = 'assets/X.svg';

const AUTO_DISMISS_MS = {
  info: 4000,
  warning: 6000,
  danger: 6000, // エラーコードを確認する時間を考慮し、他よりやや長め
  success: 4000,
};

const LEAVE_TRANSITION_MS = 250; // style.css の .toast-card の transition と合わせる
const MIN_REMAINING_AFTER_HOVER_MS = 1500; // ホバー解除後は最低これだけ見せてから消す

function getContainer() {
  let container = document.getElementById('toast-container');
  if (!container) {
    container = document.createElement('div');
    container.id = 'toast-container';
    container.className = 'toast-container';
    document.body.prepend(container);
  }
  // 読み上げの強さはカードごとの role で決める。コンテナ全体を assertive にすると
  // info/success まで読み上げを遮ってしまうため、polite にしておく。
  container.setAttribute('aria-live', 'polite');
  container.setAttribute('aria-atomic', 'false');
  return container;
}

/**
 * @param {Object} opts
 * @param {'info'|'warning'|'danger'|'success'} opts.type
 * @param {string} opts.message - ユーザー向けの説明文
 * @param {string|null} [opts.code] - バックエンドのエラーコード等。あれば表示する
 */
export function showToast({ type = 'info', message, code = null }) {
  const container = getContainer();
  const config = TYPE_CONFIG[type] || TYPE_CONFIG.info;

  const card = document.createElement('div');
  card.className = `toast-card toast-${type}`;
  card.setAttribute('role', config.role);
  card.style.setProperty('--toast-accent', `var(${config.colorVar})`);

  const icon = document.createElement('span');
  icon.className = 'toast-icon';
  icon.setAttribute('aria-hidden', 'true');
  applyMaskIcon(icon, config.icon);
  card.appendChild(icon);

  const body = document.createElement('div');
  body.className = 'toast-body';

  const messageEl = document.createElement('p');
  messageEl.className = 'toast-message';
  messageEl.textContent = message;
  body.appendChild(messageEl);

  if (code) {
    const codeEl = document.createElement('p');
    codeEl.className = 'toast-code';
    codeEl.textContent = `code: ${code}`;
    body.appendChild(codeEl);
  }
  card.appendChild(body);

  const closeBtn = document.createElement('button');
  closeBtn.type = 'button';
  closeBtn.className = 'toast-close';
  closeBtn.setAttribute('aria-label', '閉じる');
  const closeIcon = document.createElement('span');
  closeIcon.className = 'toast-close-icon';
  closeIcon.setAttribute('aria-hidden', 'true');
  applyMaskIcon(closeIcon, CLOSE_ICON);
  closeBtn.appendChild(closeIcon);
  card.appendChild(closeBtn);

  container.appendChild(card);

  const autoDismissMs = AUTO_DISMISS_MS[type] ?? 4000;
  let progress = null;
  if (autoDismissMs > 0) {
    progress = document.createElement('div');
    progress.className = 'toast-progress';
    progress.style.animationDuration = `${autoDismissMs}ms`;
    card.appendChild(progress);
  }

  // 追加した直後だとtransitionが発火しないブラウザがあるので1フレーム待ってからクラスを付ける
  requestAnimationFrame(() => {
    card.classList.add('is-visible');
  });

  let dismissed = false;
  let timer = null;

  function dismiss() {
    if (dismissed) return; // 手動で閉じたあとにタイマーが走っても二重に処理しない
    dismissed = true;
    clearTimeout(timer);

    card.classList.remove('is-visible');
    card.classList.add('is-leaving');

    // タブが非表示のときなど transitionend が来ないことがあるので、
    // タイマーでも確実に取り除く（remove() は2回呼んでも害はない）
    card.addEventListener('transitionend', () => card.remove(), { once: true });
    setTimeout(() => card.remove(), LEAVE_TRANSITION_MS + 100);
  }

  closeBtn.addEventListener('click', dismiss);

  if (autoDismissMs > 0) {
    let remainingMs = autoDismissMs;
    let startedAt = Date.now();
    timer = setTimeout(dismiss, remainingMs);

    // マウスが乗っている間（タッチ中も）は、消えるのを止めて帯の縮小も止める
    card.addEventListener('pointerenter', () => {
      if (dismissed) return;
      clearTimeout(timer);
      remainingMs -= Date.now() - startedAt;
      progress.style.animationPlayState = 'paused';
    });

    card.addEventListener('pointerleave', () => {
      if (dismissed) return;
      startedAt = Date.now();
      remainingMs = Math.max(remainingMs, MIN_REMAINING_AFTER_HOVER_MS);
      timer = setTimeout(dismiss, remainingMs);
      progress.style.animationPlayState = 'running';
    });
  }

  return { dismiss };
}