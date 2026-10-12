CREATE TABLE releases (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),

    app_id UUID NOT NULL
        REFERENCES apps(id)
        ON DELETE CASCADE,

    version VARCHAR(50) NOT NULL,

    platform VARCHAR(20) NOT NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    UNIQUE(app_id, version, platform)
);