/**
 * ヘッダーのログイン表示とアカウントメニュー（ログアウト・退会）。
 *
 * ログイン状態の本体は HttpOnly Cookie なのでJSからは読めない。表示には毎回
 * GET /auth/me の結果を使う。ログインしなくても画像編集は使えるので、確認に
 * 失敗しても編集の邪魔はせず、ログインボタンのまま(未ログイン扱い)にする。
 */
import { deleteAccount, fetchCurrentUser, logout } from './api.js';
import { showToast } from './toast.js';

const LOGIN_FAILED_MESSAGE = 'ログインできませんでした。時間をおいて再度お試しください。';
const UNSAVED_WORK_MESSAGE = 'ログインするとページを移動するため、編集中の画像は失われます。続けますか？';
const DELETE_ACCOUNT_MESSAGE =
  '退会すると、メールアドレスなどのアカウント情報が削除され、元に戻せません。\n'
  + 'ログインしなくても画像編集は引き続き利用できます。\n\n退会しますか？';
const ACCOUNT_DELETED_MESSAGE = '退会しました。ご利用ありがとうございました。';

function renderUser(elements, user) {
  const { loginLinkEl, userEl, userNameEl } = elements;
  loginLinkEl.hidden = Boolean(user);
  userEl.hidden = !user;
  userEl.open = false;
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

export async function initAuth({
  store,
  loginLinkEl,
  userEl,
  userNameEl,
  logoutBtnEl,
  deleteAccountBtnEl,
}) {
  const elements = { loginLinkEl, userEl, userNameEl };

  /**
   * ログアウトと退会の共通処理。実行中はボタンを無効にして二重送信を防ぐ。
   * 401はセッションが既に切れているということなので、表示も未ログインに揃える。
   */
  async function runAccountAction(btnEl, action) {
    btnEl.disabled = true;
    try {
      await action();
      renderUser(elements, null);
      return true;
    } catch (err) {
      if (err.status === 401) renderUser(elements, null);
      showToast({ type: 'danger', message: err.message, code: err.code });
      return false;
    } finally {
      btnEl.disabled = false;
    }
  }

  loginLinkEl.addEventListener('click', (event) => {
    // 画像はメモリ上にしか無いので、Googleへ移動すると失われる
    if (store.getState().sessions.size > 0 && !window.confirm(UNSAVED_WORK_MESSAGE)) {
      event.preventDefault();
    }
  });

  // メニューの外を押したら閉じる
  document.addEventListener('click', (event) => {
    if (!userEl.contains(event.target)) userEl.open = false;
  });

  logoutBtnEl.addEventListener('click', () => runAccountAction(logoutBtnEl, logout));

  deleteAccountBtnEl.addEventListener('click', async () => {
    if (!window.confirm(DELETE_ACCOUNT_MESSAGE)) return;

    if (await runAccountAction(deleteAccountBtnEl, deleteAccount)) {
      showToast({ type: 'success', message: ACCOUNT_DELETED_MESSAGE });
    }
  });

  consumeLoginResult();

  try {
    renderUser(elements, await fetchCurrentUser());
  } catch {
    renderUser(elements, null); // 原因は api.js が console に出している
  }
}
