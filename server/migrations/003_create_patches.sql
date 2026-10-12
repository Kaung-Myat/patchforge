CREATE TABLE patches (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),

    release_id UUID NOT NULL
        REFERENCES releases(id)
        ON DELETE CASCADE,

    patch_number INTEGER NOT NULL,

    file_path TEXT NOT NULL,

    sha256 VARCHAR(64) NOT NULL,

    file_size BIGINT NOT NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    UNIQUE(release_id, patch_number)
);