-- 動画切り抜きの 1 日あたりの利用回数。day は JST の日番号（UNIX 秒 + 9h を 86400 で割った値）。
-- key_kind は 'client'（client_id）か 'ip'（IP アドレス）。
CREATE TABLE IF NOT EXISTS video_quota_usage (
    day BIGINT NOT NULL,
    key_kind TEXT NOT NULL,
    key TEXT NOT NULL,
    count BIGINT NOT NULL,
    PRIMARY KEY (day, key_kind, key)
);
