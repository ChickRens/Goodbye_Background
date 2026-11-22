-- utm 付きの URL で来た訪問（流入元）。client_id は usage_records.client_id と同じ UUID の TEXT で、結び付けて「使ったか」を数える。
CREATE TABLE IF NOT EXISTS visit_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    client_id TEXT,
    utm_source TEXT NOT NULL,
    utm_medium TEXT,
    utm_campaign TEXT,
    utm_content TEXT,
    landing_path TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_visit_records_created_at ON visit_records (created_at);
