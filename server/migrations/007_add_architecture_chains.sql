CREATE TABLE release_artifacts (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    release_id UUID NOT NULL
        REFERENCES releases(id)
        ON DELETE CASCADE,
    architecture VARCHAR(50) NOT NULL,
    sha256 VARCHAR(64) NOT NULL,
    file_size BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(release_id, architecture)
);

INSERT INTO release_artifacts (release_id, architecture, sha256, file_size)
SELECT DISTINCT ON (release_id, architecture)
    release_id,
    architecture,
    base_sha256,
    target_file_size
FROM patches
WHERE
    architecture IS NOT NULL
    AND base_sha256 IS NOT NULL
    AND target_file_size IS NOT NULL
ORDER BY release_id, architecture, patch_number ASC;

ALTER TABLE patches
ADD COLUMN base_patch_number INTEGER;

UPDATE patches AS patch
SET base_patch_number = COALESCE(
    (
        SELECT previous.patch_number
        FROM patches AS previous
        WHERE
            previous.release_id = patch.release_id
            AND previous.architecture = patch.architecture
            AND previous.target_sha256 = patch.base_sha256
            AND previous.patch_number < patch.patch_number
        ORDER BY previous.patch_number DESC
        LIMIT 1
    ),
    0
)
WHERE
    patch.architecture IS NOT NULL
    AND patch.base_sha256 IS NOT NULL
    AND patch.target_sha256 IS NOT NULL;

ALTER TABLE patches
DROP CONSTRAINT IF EXISTS patches_release_id_patch_number_key;

CREATE UNIQUE INDEX patches_release_patch_architecture_key
ON patches (release_id, patch_number, architecture) NULLS NOT DISTINCT;

ALTER TABLE patches
ADD CONSTRAINT patches_base_patch_before_target
CHECK (base_patch_number IS NULL OR base_patch_number < patch_number);
