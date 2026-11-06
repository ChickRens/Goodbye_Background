/**
 * 送信後のジョブの状態を 1 秒ごとに取りに行く。
 *
 * - done / failed になったら止める。
 * - タブが裏に回っている間はブラウザがタイマーを間引くので、表示に戻ったら（visibilitychange）すぐ 1 回取りに行く。
 * - 404 はサーバーの再起動でジョブが消えたということなので、
 *   onUpdate({ phase: 'lost', message: SERVER_RESTARTED_MESSAGE }) を渡して止める。
 * - それ以外の失敗（通信の切断・5xx）は一時的なものとして、次の回にもう一度取りに行く。
 *   ジョブが本当に止まっていればサーバー側のタイムアウトで failed になる。
 */
import { getVideoJob } from '../api.js';
import { SERVER_RESTARTED_MESSAGE } from './messages.js';

const TERMINAL_PHASES = new Set(['done', 'failed']);

export function pollJob(jobId, onUpdate, intervalMs = 1000) {
  let stopped = false;
  let inFlight = false;
  let timer = null;

  function stop() {
    stopped = true;
    clearTimeout(timer);
    document.removeEventListener('visibilitychange', onVisibilityChange);
  }

  function schedule() {
    clearTimeout(timer);
    if (!stopped) timer = setTimeout(tick, intervalMs);
  }

  async function tick() {
    if (stopped || inFlight) return;
    clearTimeout(timer);
    inFlight = true;
    try {
      const job = await getVideoJob(jobId);
      if (stopped) return;
      onUpdate(job);
      if (TERMINAL_PHASES.has(job.phase)) stop();
    } catch (err) {
      if (stopped) return;
      if (err.status === 404) {
        stop();
        onUpdate({ phase: 'lost', message: SERVER_RESTARTED_MESSAGE });
        return;
      }
      console.warn('[video] ジョブの状態の取得に失敗。次の回にもう一度取りに行きます', err);
    } finally {
      inFlight = false;
      schedule();
    }
  }

  function onVisibilityChange() {
    if (document.visibilityState === 'visible') tick();
  }

  document.addEventListener('visibilitychange', onVisibilityChange);
  tick();
  return { stop };
}
