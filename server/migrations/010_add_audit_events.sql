CREATE TABLE audit_events (
    id BIGSERIAL PRIMARY KEY,
    actor_fingerprint VARCHAR(16) NOT NULL,
    method VARCHAR(10) NOT NULL,
    resource_path TEXT NOT NULL,
    response_status INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX audit_events_created_at_idx
ON audit_events (created_at DESC, id DESC);
