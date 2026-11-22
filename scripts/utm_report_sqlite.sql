-- 流入元（utm）の集計（SQLite。開発用。内容は utm_report_postgres.sql と同じ）。
-- 実行例: sqlite3 database/usage.db < scripts/utm_report_sqlite.sql
-- created_at は UTC で入っているので '+9 hours' で JST にする。期間は '-30 days' の 2 か所。

-- 1. 日別 × 流入元: 訪問数と訪問者数
SELECT
    date(created_at, '+9 hours') AS day_jst,
    utm_source,
    COALESCE(utm_medium, '-') AS utm_medium,
    COUNT(*) AS visits,
    COUNT(DISTINCT client_id) AS visitors
FROM visit_records
WHERE created_at >= datetime('now', '-30 days')
GROUP BY 1, 2, 3
ORDER BY 1 DESC, visits DESC;

-- 2. 流入元ごとの「使ったか」（client_id ごとに最初の訪問の流入元で数える）
WITH ranked AS (
    SELECT client_id, utm_source, utm_medium, utm_campaign, created_at,
           ROW_NUMBER() OVER (PARTITION BY client_id ORDER BY created_at, id) AS n
    FROM visit_records
    WHERE client_id IS NOT NULL
      AND created_at >= datetime('now', '-30 days')
),
first_visit AS (SELECT * FROM ranked WHERE n = 1)
SELECT
    f.utm_source,
    COALESCE(f.utm_medium, '-') AS utm_medium,
    COALESCE(f.utm_campaign, '-') AS utm_campaign,
    COUNT(*) AS visitors,
    SUM(EXISTS (SELECT 1 FROM usage_records u WHERE u.client_id = f.client_id AND u.created_at >= f.created_at
                AND u.operation = 'upload' AND u.status = 'success')) AS uploaded_image,
    SUM(EXISTS (SELECT 1 FROM usage_records u WHERE u.client_id = f.client_id AND u.created_at >= f.created_at
                AND u.operation = 'save' AND u.status = 'success')) AS saved_image,
    SUM(EXISTS (SELECT 1 FROM usage_records u WHERE u.client_id = f.client_id AND u.created_at >= f.created_at
                AND u.operation = 'start-video-job' AND u.status = 'success')) AS started_video
FROM first_visit f
GROUP BY 1, 2, 3
ORDER BY visitors DESC;
