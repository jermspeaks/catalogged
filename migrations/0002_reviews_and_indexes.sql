-- "notes" becomes the public review text.
ALTER TABLE entries RENAME COLUMN notes TO review;

-- D1 bills rows *scanned*, so list queries must be able to use an index for ORDER BY ... LIMIT.
CREATE INDEX entries_updated ON entries (updated_at DESC);
CREATE INDEX entries_kind_updated ON entries (kind, updated_at DESC);
