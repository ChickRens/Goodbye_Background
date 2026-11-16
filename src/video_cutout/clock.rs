//! 動画ジョブ用の時計。時刻が絡む処理はこの trait 越しに現在時刻を取る（テストでは `FakeVideoClock`）。

pub trait VideoClock: Send + Sync {
    /// UNIX ミリ秒。
    fn now_ms(&self) -> u64;
}
