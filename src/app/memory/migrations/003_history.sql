-- SQLite becomes the record of everything, including concepts. The journal says
-- who changed what and when, and never holds the text itself, so it can outlive
-- a record that was deleted.

ALTER TABLE notes ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;
-- Revision whose vector is in the search index; NULL when none is.
ALTER TABLE notes ADD COLUMN indexed_revision INTEGER;

CREATE INDEX notes_unindexed ON notes (id) WHERE indexed_revision IS NOT revision;

CREATE TABLE concepts (
    seq              INTEGER PRIMARY KEY AUTOINCREMENT,
    id               TEXT NOT NULL UNIQUE,
    project          TEXT NOT NULL,
    title            TEXT NOT NULL,
    content          TEXT NOT NULL,
    tags             TEXT NOT NULL,
    sources          TEXT NOT NULL CHECK (json_valid(sources) AND json_type(sources) = 'array'),
    archived         INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    author           TEXT,
    revision         INTEGER NOT NULL DEFAULT 1,
    indexed_revision INTEGER,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
) STRICT;

CREATE INDEX concepts_scope ON concepts (project, archived);
CREATE INDEX concepts_unindexed ON concepts (seq) WHERE indexed_revision IS NOT revision;

CREATE VIRTUAL TABLE concepts_fts USING fts5 (
    title, content, tags,
    content = 'concepts', content_rowid = 'seq',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER concepts_fts_insert AFTER INSERT ON concepts BEGIN
    INSERT INTO concepts_fts (rowid, title, content, tags) VALUES (new.seq, new.title, new.content, new.tags);
END;

CREATE TRIGGER concepts_fts_delete AFTER DELETE ON concepts BEGIN
    INSERT INTO concepts_fts (concepts_fts, rowid, title, content, tags) VALUES ('delete', old.seq, old.title, old.content, old.tags);
END;

CREATE TRIGGER concepts_fts_update AFTER UPDATE OF title, content, tags ON concepts BEGIN
    INSERT INTO concepts_fts (concepts_fts, rowid, title, content, tags) VALUES ('delete', old.seq, old.title, old.content, old.tags);
    INSERT INTO concepts_fts (rowid, title, content, tags) VALUES (new.seq, new.title, new.content, new.tags);
END;

INSERT INTO concepts_fts (concepts_fts, rank) VALUES ('secure-delete', 1);

-- Entities become searchable by words. The index keeps its own copy of the text.
CREATE VIRTUAL TABLE entities_fts USING fts5 (
    key, type, status, attrs,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER entities_fts_insert AFTER INSERT ON entities BEGIN
    INSERT INTO entities_fts (rowid, key, type, status, attrs)
    VALUES (new.id, new.key, new.type, new.status,
            (SELECT coalesce(group_concat(j.key || ' ' || j.value, ' '), '') FROM json_each(new.attrs) j));
END;

CREATE TRIGGER entities_fts_delete AFTER DELETE ON entities BEGIN
    DELETE FROM entities_fts WHERE rowid = old.id;
END;

CREATE TRIGGER entities_fts_update AFTER UPDATE OF key, type, status, attrs ON entities BEGIN
    DELETE FROM entities_fts WHERE rowid = old.id;
    INSERT INTO entities_fts (rowid, key, type, status, attrs)
    VALUES (new.id, new.key, new.type, new.status,
            (SELECT coalesce(group_concat(j.key || ' ' || j.value, ' '), '') FROM json_each(new.attrs) j));
END;

INSERT INTO entities_fts (entities_fts, rank) VALUES ('secure-delete', 1);

INSERT INTO entities_fts (rowid, key, type, status, attrs)
SELECT e.id, e.key, e.type, e.status,
       (SELECT coalesce(group_concat(j.key || ' ' || j.value, ' '), '') FROM json_each(e.attrs) j)
FROM entities e;

-- How many records hold a word, for weighing query words against each other.
CREATE VIRTUAL TABLE notes_vocab USING fts5vocab (notes_fts, row);
CREATE VIRTUAL TABLE concepts_vocab USING fts5vocab (concepts_fts, row);
CREATE VIRTUAL TABLE entities_vocab USING fts5vocab (entities_fts, row);

-- Full state of a note after each change. Deleted together with the note.
CREATE TABLE note_revisions (
    note_id   INTEGER NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    revision  INTEGER NOT NULL,
    status    TEXT NOT NULL,
    title     TEXT NOT NULL,
    body      TEXT NOT NULL,
    tags      TEXT NOT NULL,
    parent_id INTEGER,
    entity_id INTEGER,
    actor     TEXT,
    run       TEXT,
    origin    TEXT NOT NULL CHECK (origin IN ('recorded', 'legacy_baseline')),
    at        TEXT NOT NULL,
    PRIMARY KEY (note_id, revision)
) STRICT, WITHOUT ROWID;

CREATE TABLE concept_revisions (
    concept_id TEXT NOT NULL REFERENCES concepts (id) ON DELETE CASCADE,
    revision   INTEGER NOT NULL,
    title      TEXT NOT NULL,
    content    TEXT NOT NULL,
    tags       TEXT NOT NULL,
    sources    TEXT NOT NULL,
    archived   INTEGER NOT NULL,
    actor      TEXT,
    run        TEXT,
    origin     TEXT NOT NULL CHECK (origin IN ('recorded', 'legacy_baseline')),
    at         TEXT NOT NULL,
    PRIMARY KEY (concept_id, revision)
) STRICT, WITHOUT ROWID;

-- Append only. `fields` names what changed, never the values. task_id and
-- event_id are plain numbers on purpose: an entry outlives what it points at.
CREATE TABLE journal (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project     TEXT NOT NULL,
    record_type TEXT NOT NULL CHECK (record_type IN ('note', 'entity', 'concept', 'checkpoint')),
    record_id   TEXT NOT NULL,
    op          TEXT NOT NULL CHECK (op IN (
                    'baseline', 'created', 'updated', 'checked', 'claimed', 'released',
                    'linked', 'unlinked', 'archived', 'restored', 'redacted', 'purged')),
    revision    INTEGER,
    fields      TEXT NOT NULL CHECK (json_valid(fields) AND json_type(fields) = 'array'),
    actor       TEXT,
    run         TEXT,
    task_id     INTEGER,
    event_id    INTEGER,
    origin      TEXT NOT NULL CHECK (origin IN ('recorded', 'legacy_baseline', 'backfilled')),
    at          TEXT NOT NULL
) STRICT;

CREATE INDEX journal_record ON journal (record_type, record_id, id);
CREATE INDEX journal_project ON journal (project, id);
CREATE INDEX journal_task ON journal (task_id, id) WHERE task_id IS NOT NULL;

-- Vectors that must leave the search index; drained when the index is next used.
CREATE TABLE index_deletions (
    record_type TEXT NOT NULL,
    record_id   TEXT NOT NULL,
    PRIMARY KEY (record_type, record_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT, WITHOUT ROWID;

-- A checkpoint may belong to one task (a note and everything beneath it), and
-- remembers where the journal stood when it was saved.
ALTER TABLE checkpoints ADD COLUMN task_id INTEGER REFERENCES notes (id) ON DELETE CASCADE;
ALTER TABLE checkpoints ADD COLUMN run TEXT;
ALTER TABLE checkpoints ADD COLUMN journal_id INTEGER NOT NULL DEFAULT 0;

CREATE INDEX checkpoints_task ON checkpoints (task_id, id);

-- What was written before this version has no recorded history. Its state at
-- upgrade becomes revision 1, marked as a baseline rather than as a creation.
INSERT INTO note_revisions
    (note_id, revision, status, title, body, tags, parent_id, entity_id, actor, run, origin, at)
SELECT id, 1, status, title, body, tags, parent_id, entity_id, author, NULL, 'legacy_baseline', updated_at
FROM notes;

-- Entity events and checkpoints were recorded when they happened; they are
-- copied in, marked as backfilled.
INSERT INTO journal (project, record_type, record_id, op, revision, fields, actor, event_id, origin, at)
SELECT project, record_type, record_id, op, revision, '[]', actor, event_id, origin, at
FROM (
    SELECT project, 'note' AS record_type, CAST(id AS TEXT) AS record_id, 'baseline' AS op,
           1 AS revision, author AS actor, NULL AS event_id, 'legacy_baseline' AS origin,
           updated_at AS at
    FROM notes
    UNION ALL
    SELECT e.project, 'entity', CAST(v.entity_id AS TEXT), v.event,
           NULL, v.author, v.id, 'backfilled', v.at
    FROM entity_events v JOIN entities e ON e.id = v.entity_id
    UNION ALL
    SELECT project, 'checkpoint', CAST(id AS TEXT), 'created',
           NULL, author, NULL, 'backfilled', created_at
    FROM checkpoints
)
ORDER BY at, record_type, record_id;

UPDATE checkpoints SET journal_id = (SELECT coalesce(max(id), 0) FROM journal);
