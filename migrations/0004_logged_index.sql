-- Public lists default to "date logged": newest first, undated entries last (NULLs are smallest in SQLite,
-- so DESC puts them at the end), ties broken by last update. These indexes let a page read ~60 rows.
CREATE INDEX entries_logged ON entries (consumed_at DESC, updated_at DESC);
CREATE INDEX entries_kind_logged ON entries (kind, consumed_at DESC, updated_at DESC);
