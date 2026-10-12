ALTER TABLE patches
ADD COLUMN key_id VARCHAR(100);

UPDATE patches
SET key_id = 'main-2026'
WHERE signature IS NOT NULL;