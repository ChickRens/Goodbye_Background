-- Add migration script here
ALTER TABLE usage_records ADD COLUMN IF NOT EXISTS client_id TEXT;
