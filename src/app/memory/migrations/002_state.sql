-- Entities are structured state that the agent queries by field.

CREATE TABLE entities (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    project          TEXT NOT NULL,
    type             TEXT NOT NULL,
    key              TEXT NOT NULL,
    parent_id        INTEGER REFERENCES entities (id) ON DELETE CASCADE,
    status           TEXT NOT NULL,
    confidence       REAL CHECK (confidence IS NULL OR (confidence >= 0 AND confidence <= 1)),
    attrs            TEXT NOT NULL CHECK (json_valid(attrs) AND json_type(attrs) = 'object'),
    author           TEXT,
    -- A claim is held by whoever knows claim_id, until it expires.
    claim_id         TEXT,
    claimed_by       TEXT,
    claim_expires_at TEXT,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    UNIQUE (project, type, key),
    CHECK (parent_id <> id)
) STRICT;

CREATE INDEX entities_parent ON entities (parent_id);
CREATE INDEX entities_scope ON entities (project, type, status);

CREATE TABLE entity_edges (
    src        INTEGER NOT NULL REFERENCES entities (id) ON DELETE CASCADE,
    dst        INTEGER NOT NULL REFERENCES entities (id) ON DELETE CASCADE,
    kind       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (src, dst, kind),
    CHECK (src <> dst)
) STRICT, WITHOUT ROWID;

CREATE INDEX entity_edges_dst ON entity_edges (dst);

-- Latest result per check; history lives in entity_events.
CREATE TABLE entity_checks (
    entity_id  INTEGER NOT NULL REFERENCES entities (id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    result     TEXT NOT NULL,
    detail     TEXT,
    author     TEXT,
    checked_at TEXT NOT NULL,
    PRIMARY KEY (entity_id, name)
) STRICT, WITHOUT ROWID;

-- Append only.
CREATE TABLE entity_events (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_id INTEGER NOT NULL REFERENCES entities (id) ON DELETE CASCADE,
    event     TEXT NOT NULL CHECK (event IN ('created', 'updated', 'checked', 'claimed', 'released')),
    detail    TEXT NOT NULL CHECK (json_valid(detail)),
    author    TEXT,
    at        TEXT NOT NULL
) STRICT;

CREATE INDEX entity_events_entity ON entity_events (entity_id, id);

CREATE TABLE checkpoints (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    project    TEXT NOT NULL,
    summary    TEXT NOT NULL,
    author     TEXT,
    created_at TEXT NOT NULL
) STRICT;

CREATE INDEX checkpoints_project ON checkpoints (project, id);

ALTER TABLE notes ADD COLUMN author TEXT;
ALTER TABLE notes ADD COLUMN entity_id INTEGER REFERENCES entities (id) ON DELETE SET NULL;

CREATE INDEX notes_entity ON notes (entity_id);

-- Wipe deleted text from the full-text index too.
INSERT INTO notes_fts (notes_fts, rank) VALUES ('secure-delete', 1);
