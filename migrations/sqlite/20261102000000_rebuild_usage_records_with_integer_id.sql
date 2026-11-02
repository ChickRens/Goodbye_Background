-- SQLite 専用。最初のマイグレーションは Postgres 用の型（BIGSERIAL / TIMESTAMPTZ）で書かれていたため、
-- SQLite では id が rowid の別名にならず、INSERT 時に NULL のままだった。
-- 適用済みマイグレーションは書き換えられない（チェックサム不一致で起動失敗する）ので、
-- テーブルを作り直して既存行をコピーし、id を振り直す。
CREATE TABLE usage_records_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    operation TEXT NOT NULL,
    status TEXT NOT NULL,
    failure_cause TEXT,
    processing_time_ms INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    client_id TEXT
);

INSERT INTO usage_records_new (operation, status, failure_cause, processing_time_ms, created_at, client_id)
SELECT operation, status, failure_cause, processing_time_ms, created_at, client_id
FROM usage_records
ORDER BY rowid;

DROP TABLE usage_records;

ALTER TABLE usage_records_new RENAME TO usage_records;
