# 流入元（UTM）の記録と集計 — 設計（短）

## 目的
YouTube などから `?utm_source=youtube&utm_medium=longform` 付きの URL で来た人を数え、
さらに「その人が実際に使ったか（画像の保存・動画の開始）」まで見られるようにする（課金の検討の材料）。

## 現状
- ページの表示はどこにも記録されていない（HTML は `ServeDir` がそのまま返す。`usage_records` は API だけ）。外部のアクセス解析も無い。
- `client-id`（ブラウザごとの UUID）は `usage_records.client_id` に入っている → 訪問にも同じ ID を記録すれば結び付けられる。

## 方式（オーナーが A を選んだ）
- **A. 自前の API + テーブル（採用）**: 使ったかまで client_id で追える。外部サービス不要。
- B. Cloud Run のアクセスログを集計: コード不要だが、使ったかと結び付かず保存期間も短い。
- C. Google Analytics: 外部スクリプトとプライバシーポリシーの変更が大きく、この用途には大げさ。

## 設計
1. フロント（index / app / video の 3 ページ）: URL に `utm_source` があれば `POST /track-visit` に
   `{utm_source, utm_medium, utm_campaign, utm_content, landing_path}` を `client-id` ヘッダー付きで送る。
   送ったら `history.replaceState` で URL から `utm_*` を消す（リロード・共有で二重に数えない）。失敗しても画面には何も出さない。
2. サーバー: `visit_records` テーブル（id / client_id / utm_source / utm_medium / utm_campaign / utm_content / landing_path / created_at）。
   SQLite・Postgres の両方にマイグレーションを追加。記録は `UsageRecorder` と同じく mpsc → 別タスクで INSERT。
3. `/track-visit` は `track_usage` の外（`usage_records` には入れない）。返事は常に 204。
   値は前後の空白を除いて最大 100 文字、`utm_source`・`utm_medium` は小文字にそろえる。`utm_source` が空なら捨てる。
   `landing_path` は `/` で始まる 200 文字以内のパスだけ（それ以外は `/`）。
   ルーターのテストをモデル無しで書けるよう、`App` には入れず `Arc<dyn VisitRecorder>` を State にした別の Router にする（動画の R4 と同じ理由。判断: Claude）。
4. 集計 SQL を `scripts/utm_report_{postgres,sqlite}.sql` に置く（日別・流入元別の訪問数・訪問者数、そのうち画像を保存した人・動画を開始した人）。
5. プライバシーポリシーに流入元の記録を追記。

## やらないこと
- utm の無い訪問（直接・検索）の記録、リファラーの記録。
- 水増し対策（誰でも叩ける。今の規模では気にしない）。
- 管理画面。集計は SQL を手で流す。
