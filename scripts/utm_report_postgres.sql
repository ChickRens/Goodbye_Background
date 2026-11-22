-- 流入元（utm）の集計（PostgreSQL。本番用）。visit_records は /track-visit が記録する（src/analytics/visit_tracking/）。
-- 実行例: psql "$DATABASE_URL_POSTGRES" -f scripts/utm_report_postgres.sql
-- 日付は JST。期間を変えるときは下の 2 か所の INTERVAL '30 days' を書き換える。

-- 1. 日別 × 流入元: 訪問数と訪問者数（同じブラウザ = 同じ client_id は 1 人）
SELECT
    (created_at AT TIME ZONE 'Asia/Tokyo')::date AS day_jst,
    utm_source,
    COALESCE(utm_medium, '-') AS utm_medium,
    COUNT(*) AS visits,
    COUNT(DISTINCT client_id) AS visitors
FROM visit_records
WHERE created_at >= NOW() - INTERVAL '30 days'
GROUP BY 1, 2, 3
ORDER BY 1 DESC, visits DESC;

-- 2. 流入元ごとの「使ったか」: 初めて来た後に、画像を保存した人・動画を開始した人（どちらも成功したものだけ）
--    client_id ごとに最初の訪問だけを流入元として数える（2 回目以降に別の utm で来ても、最初の流入元のまま）
WITH first_visit AS (
    SELECT DISTINCT ON (client_id)
        client_id, utm_source, utm_medium, utm_campaign, created_at
    FROM visit_records
    WHERE client_id IS NOT NULL
      AND created_at >= NOW() - INTERVAL '30 days'
    ORDER BY client_id, created_at
)
SELECT
    f.utm_source,
    COALESCE(f.utm_medium, '-') AS utm_medium,
    COALESCE(f.utm_campaign, '-') AS utm_campaign,
    COUNT(*) AS visitors,
    COUNT(*) FILTER (WHERE EXISTS (
        SELECT 1 FROM usage_records u
        WHERE u.client_id = f.client_id AND u.created_at >= f.created_at
          AND u.operation = 'upload' AND u.status = 'success'
    )) AS uploaded_image,
    COUNT(*) FILTER (WHERE EXISTS (
        SELECT 1 FROM usage_records u
        WHERE u.client_id = f.client_id AND u.created_at >= f.created_at
          AND u.operation = 'save' AND u.status = 'success'
    )) AS saved_image,
    COUNT(*) FILTER (WHERE EXISTS (
        SELECT 1 FROM usage_records u
        WHERE u.client_id = f.client_id AND u.created_at >= f.created_at
          AND u.operation = 'start-video-job' AND u.status = 'success'
    )) AS started_video
FROM first_visit f
GROUP BY 1, 2, 3
ORDER BY visitors DESC;
