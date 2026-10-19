/**
 * 状態は localId（クライアント生成のUUID）をキーにしたMapで持つ。
 *
 * なぜ image_id ではなく localId をキーにするか:
 * - アップロード完了(=サーバーがimage_idを発行する)前から、ギャラリーに
 *   サムネイルを表示したい。サーバーIDが来るまでキーが存在しない状態を
 *   避けるため、ファイル選択の瞬間にクライアント側でIDを払い出す。
 * - image_id は session.imageId フィールドとして持ち、アップロード完了後に埋める。
 */
export function createStore() {
  const state = {
    sessions: new Map(), // localId -> session
    activeLocalId: null,
  };
  const listeners = new Set();

  function notify() {
    listeners.forEach((fn) => fn(state));
  }

  return {
    getState() {
      return state;
    },

    subscribe(fn) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },

    createSession(localId, extra = {}) {
      state.sessions.set(localId, {
        localId,
        sessionId: null, // /upload のレスポンスが来るまで null（undo/redoの対象）
        imageId: null, // 現在表示すべきスナップショットのポインタ（get-preview用）
        status: 'uploading', // uploading | ready | editing | saving | saved | error
        // マーカー表示用のローカルなポイント履歴。
        // 注意: undo/redoはサーバーがpointsを返さないため、
        // 「1回のundo/redo = 1点」という前提のベストエフォート同期でしかない。
        // 実際に表示される画像はget-previewで毎回取得し直すので必ず正しいが、
        // マーカーの見た目だけはこの前提が崩れるとズレ得る。
        points: [],
        redoStack: [], // undoで取り除いた点。redoで戻すための一時退避
        // バックエンドに一般的なversionは無いが、image_idが変わること自体が
        // 実質的なversionの役割を果たす。とはいえ複数の変更系リクエストが
        // 同時に飛ぶと「どちらのimage_idが最終的に正しいか」は分からないため、
        // busy はそのための直列化ロックとして引き続き必要。
        busy: false,
        thumbUrl: null, // ギャラリー用（ローカルのFileから生成、ネットワーク不要）
        fullUrl: null, // 編集画面用（/get-preview の結果。差し替え時に revoke する）
        naturalWidth: 0,
        naturalHeight: 0,
        error: null,
        file: null,
        ...extra,
      });
      notify();
    },

    updateSession(localId, patch) {
      const session = state.sessions.get(localId);
      if (!session) return; // 削除済み/存在しないセッションへの遅延レスポンスは握りつぶす
      Object.assign(session, patch);
      notify();
    },

    /**
     * セッションをストアから取り除き、持っているBlob URLを解放する。
     * （現状はアップロード失敗したサムネイルをユーザーが消すときに使う）
     */
    removeSession(localId) {
      const session = state.sessions.get(localId);
      if (!session) return;
      if (session.thumbUrl) URL.revokeObjectURL(session.thumbUrl);
      if (session.fullUrl) URL.revokeObjectURL(session.fullUrl);
      state.sessions.delete(localId);
      if (state.activeLocalId === localId) state.activeLocalId = null;
      notify();
    },

    getSession(localId) {
      return state.sessions.get(localId);
    },

    setActive(localId) {
      state.activeLocalId = localId;
      notify();
    },
  };
}