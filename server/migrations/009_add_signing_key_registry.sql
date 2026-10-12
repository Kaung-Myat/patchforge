CREATE TABLE signing_keys (
    key_id VARCHAR(100) PRIMARY KEY,
    public_key_base64 TEXT NOT NULL DEFAULT '',
    status VARCHAR(20) NOT NULL DEFAULT 'retired',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT signing_keys_status_valid
        CHECK (status IN ('active', 'retired', 'revoked'))
);

INSERT INTO signing_keys (key_id, status)
SELECT DISTINCT key_id, 'retired'
FROM patches
WHERE key_id IS NOT NULL
ON CONFLICT (key_id) DO NOTHING;
