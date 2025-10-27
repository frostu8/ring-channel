-- Adds some columns to track the gain and loss of mobiums
ALTER TABLE user ADD COLUMN mobiums_gained BIGINT NOT NULL DEFAULT 0;
ALTER TABLE user ADD COLUMN mobiums_lost BIGINT NOT NULL DEFAULT 0;
