/**
 * ヘッダーのログイン表示。
 *
 * ログイン状態の本体は HttpOnly Cookie なのでJSからは読めない。表示には毎回
 * GET /auth/me の結果を使う。ログインしなくても画像編集は使えるので、確認に
 * 失敗しても編集の邪魔はせず、ログインボタンのまま(未ログイン扱い)にする。
 */
import { fetchCurrentUser, logout } from './api.js';
import { showToast } from './toast.js';

const LOGIN_FAILED_MESSAGE = 'ログインできませんでした。時間をおいて再度お試しください。';
const UNSAVED_WORK_MESSAGE = 'ログインするとページを移動するため、編集中の画像は失われます。続けますか？';

function renderUser(elements, user) {
  const { loginLinkEl, userEl, userNameEl } = elements;
  loginLinkEl.hidden = Boolean(user);
  userEl.hidden = !user;
  userNameEl.textContent = user ? (user.display_name ?? user.email) : '';
  userNameEl.title = user ? user.email : '';
}

// ログイン失敗時はサーバーが app.html?login=failed へ戻す。
// リロードで再度トーストが出ないよう、見たらURLから取り除く。
function consumeLoginResult() {
  const url = new URL(window.location.href);
  if (url.searchParams.get('login') !== 'failed') return;

  url.searchParams.delete('login');
  window.history.replaceState(null, '', url);
  showToast({ type: 'danger', message: LOGIN_FAILED_MESSAGE });
}

export async function initAuth({ store, loginLinkEl, userEl, userNameEl, logoutBtnEl }) {
  const elements = { loginLinkEl, userEl, userNameEl };

  loginLinkEl.addEventListener('click', (event) => {
    // 画像はメモリ上にしか無いので、Googleへ移動すると失われる
    if (store.getState().sessions.size > 0 && !window.confirm(UNSAVED_WORK_MESSAGE)) {
      event.preventDefault();
    }
  });

  logoutBtnEl.addEventListener('click', async () => {
    logoutBtnEl.disabled = true;
    try {
      await logout();
      renderUser(elements, null);
    } catch (err) {
      showToast({ type: 'danger', message: err.message, code: err.code });
    } finally {
      logoutBtnEl.disabled = false;
    }
  });

  consumeLoginResult();

  try {
    renderUser(elements, await fetchCurrentUser());
  } catch {
    renderUser(elements, null); // 原因は api.js が console に出している
  }
}
