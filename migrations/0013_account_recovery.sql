-- Recovery secrets are hashed; queued email contents are encrypted by the API.
ALTER TABLE users ADD COLUMN email_verified_at TIMESTAMPTZ;

CREATE TABLE account_recovery_tokens (
    id UUID PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    purpose TEXT NOT NULL CHECK(purpose IN ('reset','verify')),
    email_snapshot TEXT NOT NULL CHECK(length(email_snapshot) BETWEEN 3 AND 254),
    token_hash BYTEA UNIQUE NOT NULL CHECK(octet_length(token_hash)=32),
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK(expires_at>created_at)
);
CREATE INDEX account_recovery_user_purpose ON account_recovery_tokens(user_id,purpose);
CREATE INDEX account_recovery_expiry ON account_recovery_tokens(expires_at);
CREATE INDEX account_recovery_retention ON account_recovery_tokens((COALESCE(consumed_at,expires_at)));

-- Hash the recipient so throttling unknown addresses does not store their email.
-- A row lock makes the limits apply across API replicas.
CREATE TABLE account_recovery_limits (
    recipient_hash BYTEA PRIMARY KEY CHECK(octet_length(recipient_hash)=32),
    window_started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    requests INTEGER NOT NULL DEFAULT 0 CHECK(requests BETWEEN 0 AND 5),
    last_requested_at TIMESTAMPTZ
);
CREATE INDEX account_recovery_limits_window ON account_recovery_limits(window_started_at);

CREATE TABLE mail_jobs (
    id UUID PRIMARY KEY,
    token_id UUID NOT NULL REFERENCES account_recovery_tokens ON DELETE CASCADE,
    user_id BIGINT NOT NULL REFERENCES users ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK(kind IN ('reset','verify')),
    encrypted_payload BYTEA,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','sending','sent','failed','canceled')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 8),
    available_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    lease_until TIMESTAMPTZ,
    lease_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    sent_at TIMESTAMPTZ,
    last_error_code TEXT CHECK(length(last_error_code)<=64),
    CHECK(encrypted_payload IS NULL OR octet_length(encrypted_payload) BETWEEN 41 AND 1065),
    CHECK(status NOT IN ('pending','sending') OR encrypted_payload IS NOT NULL),
    CHECK(status<>'sending' OR (lease_until IS NOT NULL AND lease_id IS NOT NULL)),
    CHECK(status<>'sent' OR sent_at IS NOT NULL)
);
CREATE INDEX mail_jobs_pending ON mail_jobs(available_at,created_at) WHERE status='pending';
CREATE INDEX mail_jobs_lease ON mail_jobs(lease_until) WHERE status='sending';
CREATE INDEX mail_jobs_token ON mail_jobs(token_id);
