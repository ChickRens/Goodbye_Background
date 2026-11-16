//! ワーカー通知の反映のエラー。返却・ファイル削除の失敗はログだけなので、ここには出ない。

use crate::{parent_error, video_cutout::job::error::VideoJobError};

parent_error!(
    pub enum ReportVideoJobEventError {
        Job(VideoJobError),
    }
);
