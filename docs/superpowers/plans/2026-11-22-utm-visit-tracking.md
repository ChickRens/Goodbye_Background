# 流入元（UTM）の記録と集計 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** utm 付きの URL で来た訪問を `visit_records` に記録し、流入元ごとの訪問数と「使ったか」を SQL で集計できるようにする。

**Architecture:** フロントが `POST /track-visit` に utm を送る → handler が `VisitEvent` に整えて `VisitRecorder::record`（mpsc に try_send）→ 別タスクで INSERT。
`/track-visit` は `Arc<dyn VisitRecorder>` を State にした独立の Router（`App` 無しでテストできる）で、`track_usage` の外に merge する。

**Tech Stack:** Rust / axum 0.8 / sqlx（SQLite・Postgres）/ Vanilla JS

**Spec:** `docs/superpowers/specs/2026-11-22-utm-visit-tracking-design.md`

## Global Constraints
- utm の各値: 前後の空白を除き最大 100 文字（文字数で切る）。`utm_source`・`utm_medium` は小文字。空は NULL。`utm_source` が空なら記録しない。
- `landing_path`: `/` で始まり 200 文字以内ならそのまま、それ以外は `/`。
- `/track-visit` は常に 204。`usage_records` には入れない。
- 適用済みマイグレーションは編集しない。新しいファイルを `migrations/sqlite/` と `migrations/postgres/` の両方に足す。
- コメント・ドキュメントは日本語。

## Review Focus
- `client-id` ヘッダーが無い/不正 → client_id を NULL にして記録する（捨てない）。→ Task 2 のテスト
- 本文が JSON でない/フィールド欠け → axum が 4xx を返してよい。フロントは失敗を無視する。
- 絵文字など多バイトの値 → バイトではなく文字数で切り、panic しない。→ Task 1 のテスト
- utm を消した後の URL にハッシュや他のクエリが残る → `replaceState` で utm_* 以外はそのまま残す。→ Task 3 の手動確認
- チャネルが満杯 → eprintln して捨てる（レスポンスに影響しない）。

---

### Task 1: VisitEvent・VisitRecorder・マイグレーション

**Files:**
- Create: `src/analytics/visit_tracking/{mod.rs,event.rs,visit_recorder.rs,recorder.rs}`
- Modify: `src/analytics/mod.rs`
- Create: `migrations/sqlite/20261122000000_visit_records.sql`, `migrations/postgres/20261122000000_visit_records.sql`

**Interfaces:**
- Produces: `VisitEvent::new(client_id: Option<Uuid>, utm_source: Option<&str>, utm_medium: Option<&str>, utm_campaign: Option<&str>, utm_content: Option<&str>, landing_path: Option<&str>) -> Option<VisitEvent>`（`utm_source` が空なら None）とゲッター。
  `trait VisitRecorder: Send + Sync { fn record(&self, event: VisitEvent); }`、`SqliteVisitRecorder::new(SqlitePool)`、`PostgresVisitRecorder::new(PgPool)`。

- [ ] テスト（`event.rs` の `visit_event_test`）: 空白除去と小文字化（`" YouTube "` → `"youtube"`、campaign は大文字のまま）、`utm_source` が空/None → None、101 文字 → 100 文字、絵文字 101 個 → 100 個、`landing_path` が `"app.html"`・201 文字・None → `"/"`。
- [ ] テスト（`recorder.rs`）: メモリの SQLite にマイグレーションを流し、`SqliteVisitRecorder` で 1 件 record → 行が入る（ポーリングで最大 1 秒待つ）。
- [ ] 実装 → `cargo test visit` が通る → commit。

### Task 2: `/track-visit` のルート

**Files:**
- Create: `src/api/handler/analytics/{mod.rs,track_visit/{mod.rs,handler.rs,request.rs}}`
- Modify: `src/api/handler/mod.rs`, `src/presentation/router.rs`（`route` に引数 `visits: Arc<dyn VisitRecorder>`、`visit_routes(visits) -> Router`）, `src/main.rs`（両 DB で VisitRecorder を作る）

**Interfaces:**
- Consumes: Task 1 の `VisitEvent::new`・`VisitRecorder`。
- Produces: `pub fn visit_routes(visits: Arc<dyn VisitRecorder>) -> Router`。

- [ ] テスト（`router_test`）: 偽の recorder（`Mutex<Vec<VisitEvent>>`）で `POST /track-visit` → 204 で 1 件。client-id 無し → 204 で client_id None。`utm_source` 無し → 204 で 0 件。
- [ ] 実装 → `cargo test` 全体が通る → commit。

### Task 3: フロント・集計 SQL・プライバシーポリシー

**Files:**
- Create: `frontend/js/visit.js`（`trackVisit()`）, `scripts/utm_report_postgres.sql`, `scripts/utm_report_sqlite.sql`
- Modify: `frontend/js/api.js`（`postVisit(body)`）, `frontend/js/home.js`・`app.js`・`video/page.js`（先頭で `trackVisit()`）, `frontend/privacy.html`

- [ ] `trackVisit()`: `location.search` に `utm_source` があれば `postVisit` を投げっぱなし（`keepalive: true`、catch で握りつぶす）、`history.replaceState` で utm_* だけ消す。
- [ ] 集計 SQL: (1) 日別×source×medium の訪問数・訪問者数（client_id の重複除き）(2) source×medium×campaign ごとの訪問者数と、そのうち訪問後に `save` / `start-video-job` が success だった人数。
- [ ] 確認: ローカルで `?utm_source=youtube&utm_medium=longform` を開き、URL から utm が消え、`visit_records` に 1 行入り、集計 SQL が結果を返す。→ commit・push。
