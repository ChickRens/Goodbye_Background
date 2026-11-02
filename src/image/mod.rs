//! 画像ドメイン。用途ごとに別の型・ストアを持つ:
//!
//! | サブモジュール | 何の画像か | サイズ | 形式 |
//! |---|---|---|---|
//! | `original` | アップロードされた元画像 | 元のまま | 生 RGBA |
//! | `segmenter_input` | SAM2 に入れる画像 | 1024x1024 固定（アスペクト比無視で引き伸ばし） | 生 RGBA |
//! | `preview` | 画面表示用（元画像 / 切り抜き結果） | 長辺 600px | WebP |
//! | `completed` | 保存（ダウンロード）用の最終結果 | 元のまま | WebP (quality 95, method 2) |
//!
//! 共通の器は `image::image::Image`（バイト列 + ImageId + サイズ）。

pub mod completed;
pub mod image;
pub mod original;
pub mod preview;
pub mod segmenter_input;
