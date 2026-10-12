ALTER TABLE releases
ADD COLUMN channel VARCHAR(20) NOT NULL DEFAULT 'stable';

ALTER TABLE releases
ADD CONSTRAINT releases_channel_valid
CHECK (channel IN ('development', 'staging', 'beta', 'stable'));

ALTER TABLE releases
DROP CONSTRAINT IF EXISTS releases_app_id_version_platform_key;

ALTER TABLE releases
ADD CONSTRAINT releases_app_version_platform_channel_key
UNIQUE (app_id, version, platform, channel);

ALTER TABLE patches
ADD COLUMN manifest_version INTEGER NOT NULL DEFAULT 1,
ADD COLUMN rollout_percentage INTEGER NOT NULL DEFAULT 100;

ALTER TABLE patches
ADD CONSTRAINT patches_manifest_version_valid
CHECK (manifest_version IN (1, 2)),
ADD CONSTRAINT patches_rollout_percentage_valid
CHECK (rollout_percentage BETWEEN 0 AND 100);
