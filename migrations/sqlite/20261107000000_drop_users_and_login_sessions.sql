-- Google ログインのテーブルを消す（オーナー判断 2026-11-07: Google ログインを外したので個人情報を残さない）。
-- users / login_sessions は 20261101090000（feat/user-google-login ブランチ由来。本番 DB に適用済み）で作られた。
-- 索引 idx_login_sessions_expires_at はテーブルと一緒に消える。login_sessions が users を参照するので先に消す。
DROP TABLE IF EXISTS login_sessions;
DROP TABLE IF EXISTS users;
