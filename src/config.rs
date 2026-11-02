//! アプリ全体の定数設定。
//!
//! - `MAX_HISTORY`: undo できる最大回数（ポイント履歴/推論コンテキスト履歴の両方に適用）
//! - `CLEANUP_INTERVAL`: 期限切れエントリ削除(GC)タスクの実行間隔
//! - `TTL_SEC`: 全インメモリストアの有効期限。「最後にアクセスされてから」この時間が経つと GC で消える
//!
//! 注意: `rule.txt` には「セッション寿命30分」と書かれているが、実装は 10 分（`TTL_SEC`）。

use std::time::Duration;

pub const MAX_HISTORY: usize = 40;
pub const CLEANUP_INTERVAL: Duration = Duration::new(60, 0);
pub const TTL_SEC: Duration = Duration::new(10 * 60, 0);
