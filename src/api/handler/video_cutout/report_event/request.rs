//! `POST /internal/video-jobs/{id}/events` のリクエスト。
//!
//! 本文は `WorkerEvent` の JSON（`contracts/video_cutout/worker_event_*.json`）。署名を本文の生のバイト列で
//! 検証してから JSON として読むので、axum の `Json` 抽出子は使わない。
//!
//! ヘッダー: `X-Signature-Timestamp`（UNIX 秒）と `X-Signature`（`hex(HMAC-SHA256(secret, "{ts}." + 本文))`）。

pub const SIGNATURE_TIMESTAMP_HEADER: &str = "x-signature-timestamp";
pub const SIGNATURE_HEADER: &str = "x-signature";
