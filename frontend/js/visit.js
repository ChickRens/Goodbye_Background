/**
 * 流入元（utm 付きの URL）の記録。index / app / video の各エントリーポイントの先頭で trackVisit() を呼ぶ。
 *
 * 例: https://chickren.goodbye-background.com/?utm_source=youtube&utm_medium=longform
 *   → POST /track-visit { utm_source: "youtube", utm_medium: "longform", ..., landing_path: "/" }
 *   → URL から utm_* だけを消す（リロードやリンクの共有で二重に数えないため。ほかのクエリ・ハッシュは残す）
 * utm_source が無いときは何もしない。サーバー側で値を整えて visit_records に入れる（src/analytics/visit_tracking/）。
 * 集計は scripts/utm_report_{postgres,sqlite}.sql。
 */
import { postVisit } from './api.js?v=1.6.2';

const UTM_KEYS = ['utm_source', 'utm_medium', 'utm_campaign', 'utm_content'];

/** location.search から送る本文を作る。utm_source が無ければ null。 */
export function visitFromSearch(search, pathname) {
  const params = new URLSearchParams(search);
  if (!params.get('utm_source')) return null;
  const body = { landing_path: pathname };
  for (const key of UTM_KEYS) body[key] = params.get(key);
  return body;
}

/** location.search から utm_* だけを消したもの（"?a=1" か ""）。 */
export function searchWithoutUtm(search) {
  const params = new URLSearchParams(search);
  for (const key of [...params.keys()]) {
    if (key.startsWith('utm_')) params.delete(key);
  }
  const rest = params.toString();
  return rest ? `?${rest}` : '';
}

export function trackVisit() {
  const body = visitFromSearch(location.search, location.pathname);
  if (!body) return;
  postVisit(body);
  history.replaceState(history.state, '', `${location.pathname}${searchWithoutUtm(location.search)}${location.hash}`);
}
