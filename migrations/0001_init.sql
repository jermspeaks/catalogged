CREATE TABLE entries (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  kind         TEXT NOT NULL CHECK (kind IN ('game','book','show','movie')),
  slug         TEXT NOT NULL,
  title        TEXT NOT NULL,
  status       TEXT NOT NULL CHECK (status IN ('wanted','in_progress','consumed')),
  rating       INTEGER CHECK (rating BETWEEN 1 AND 10),
  notes        TEXT,
  year         INTEGER,
  source       TEXT CHECK (source IN ('tmdb','igdb','open_library')),
  external_id  TEXT,
  cover_key    TEXT,
  consumed_at  TEXT,
  created_at   TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at   TEXT NOT NULL DEFAULT (datetime('now')),
  UNIQUE (kind, slug)
);

-- A given upstream item can only be catalogued once.
CREATE UNIQUE INDEX entries_external ON entries (kind, source, external_id)
  WHERE source IS NOT NULL AND external_id IS NOT NULL;

CREATE INDEX entries_status ON entries (kind, status);
