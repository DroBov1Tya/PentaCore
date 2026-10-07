-- parent_id forms the goal tree, edges carry the typed links between notes.

CREATE TABLE IF NOT EXISTS notes (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    project    TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('goal', 'step', 'attempt', 'fact', 'decision', 'question')),
    status     TEXT NOT NULL CHECK (status IN ('open', 'active', 'done', 'failed', 'dropped')),
    title      TEXT NOT NULL,
    body       TEXT NOT NULL,
    tags       TEXT NOT NULL,
    parent_id  INTEGER REFERENCES notes (id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (kind <> 'goal' OR parent_id IS NULL),
    CHECK (parent_id <> id)
) STRICT;

CREATE INDEX IF NOT EXISTS notes_parent ON notes (parent_id);
CREATE INDEX IF NOT EXISTS notes_scope ON notes (project, kind, status);
CREATE INDEX IF NOT EXISTS notes_recent ON notes (updated_at);

CREATE TABLE IF NOT EXISTS edges (
    src        INTEGER NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    dst        INTEGER NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('depends_on', 'supports', 'contradicts', 'answers', 'supersedes', 'relates_to')),
    created_at TEXT NOT NULL,
    PRIMARY KEY (src, dst, kind),
    CHECK (src <> dst)
) STRICT, WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS edges_dst ON edges (dst);

CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5 (
    title, body, tags,
    content = 'notes', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS notes_fts_insert AFTER INSERT ON notes BEGIN
    INSERT INTO notes_fts (rowid, title, body, tags) VALUES (new.id, new.title, new.body, new.tags);
END;

CREATE TRIGGER IF NOT EXISTS notes_fts_delete AFTER DELETE ON notes BEGIN
    INSERT INTO notes_fts (notes_fts, rowid, title, body, tags) VALUES ('delete', old.id, old.title, old.body, old.tags);
END;

CREATE TRIGGER IF NOT EXISTS notes_fts_update AFTER UPDATE OF title, body, tags ON notes BEGIN
    INSERT INTO notes_fts (notes_fts, rowid, title, body, tags) VALUES ('delete', old.id, old.title, old.body, old.tags);
    INSERT INTO notes_fts (rowid, title, body, tags) VALUES (new.id, new.title, new.body, new.tags);
END;

