-- Sessions are now looked up by the SHA-256 hash of the cookie token instead
-- of the raw token, so a leaked database dump contains no usable sessions.
--
-- Existing rows used the row id itself as the cookie token; hashing that id
-- with the exact same encoding the backend uses (lowercase hex of the SHA-256
-- of the hyphenated lowercase UUID string, see session_token_hash) keeps every
-- session issued before this migration valid.
ALTER TABLE sessions ADD COLUMN token_hash TEXT;
UPDATE sessions SET token_hash = encode(sha256(convert_to(id::text, 'UTF8')), 'hex');
ALTER TABLE sessions ALTER COLUMN token_hash SET NOT NULL;
CREATE UNIQUE INDEX sessions_token_hash_key ON sessions (token_hash);
