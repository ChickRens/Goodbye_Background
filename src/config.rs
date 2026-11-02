//! アプリ全体の定数設定。
//!
//! - `MAX_HISTORY`: undo できる最大回数（ポイント履歴/推論コンテキスト履歴の両方に適用）
//! - `CLEANUP_INTERVAL`: 期限切れエントリ削除(GC)タスクの実行間隔
//! - `TTL_SEC`: 全インメモリストアの有効期限。「最後にアクセスされてから」この時間が経つと GC で消える
//!
//! `TTL_SEC` / `CLEANUP_INTERVAL` を変えたら `rule.txt` と `frontend/privacy.html`（保存期間の記載）も更新すること。

use std::time::Duration;

pub const MAX_HISTORY: usize = 40;
pub const CLEANUP_INTERVAL: Duration = Duration::new(60, 0);
pub const TTL_SEC: Duration = Duration::new(10 * 60, 0);
