-- Journal, revisions, checklists, task summaries, confidence, lessons, vectors.

-- notes is rebuilt, because a new kind cannot be added to its CHECK in place.
-- This file runs with foreign keys off, as SQLite's rebuild procedure requires;
-- the links are verified before the migration commits (see db.rs).
CREATE TABLE notes_new (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    project           TEXT NOT NULL,
    kind              TEXT NOT NULL CHECK (kind IN ('goal', 'step', 'attempt', 'fact', 'decision', 'question', 'lesson')),
    status            TEXT NOT NULL CHECK (status IN ('open', 'active', 'done', 'failed', 'dropped')),
    title             TEXT NOT NULL,
    body              TEXT NOT NULL,
    tags              TEXT NOT NULL,
    parent_id         INTEGER REFERENCES notes (id) ON DELETE CASCADE,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    author            TEXT,
    entity_id         INTEGER REFERENCES entities (id) ON DELETE SET NULL,
    revision          INTEGER NOT NULL DEFAULT 1,
    -- 1 for the one closing summary of the task this note sits under.
    summary           INTEGER NOT NULL DEFAULT 0 CHECK (summary IN (0, 1)),
    -- How sure the author was, why, and when someone last checked it.
    confidence        REAL CHECK (confidence IS NULL OR (confidence >= 0 AND confidence <= 1)),
    basis             TEXT,
    verified_at       TEXT,
    verified_by       TEXT,
    -- Vector for search by meaning, and the revision it was computed from.
    embedding         BLOB,
    embedded_revision INTEGER,
    CHECK (kind <> 'goal' OR parent_id IS NULL),
    CHECK (parent_id <> id)
) STRICT;

INSERT INTO notes_new
    (id, project, kind, status, title, body, tags, parent_id, created_at, updated_at, author, entity_id)
SELECT id, project, kind, status, title, body, tags, parent_id, created_at, updated_at, author, entity_id
FROM notes;

DROP TABLE notes;
ALTER TABLE notes_new RENAME TO notes;

CREATE INDEX notes_parent ON notes (parent_id);
CREATE INDEX notes_scope ON notes (project, kind, status);
CREATE INDEX notes_recent ON notes (updated_at);
CREATE INDEX notes_entity ON notes (entity_id);
CREATE UNIQUE INDEX notes_summary ON notes (parent_id) WHERE summary = 1;
CREATE INDEX notes_unembedded ON notes (id) WHERE embedded_revision IS NOT revision;

CREATE TRIGGER notes_fts_insert AFTER INSERT ON notes BEGIN
    INSERT INTO notes_fts (rowid, title, body, tags) VALUES (new.id, new.title, new.body, new.tags);
END;

CREATE TRIGGER notes_fts_delete AFTER DELETE ON notes BEGIN
    INSERT INTO notes_fts (notes_fts, rowid, title, body, tags) VALUES ('delete', old.id, old.title, old.body, old.tags);
END;

CREATE TRIGGER notes_fts_update AFTER UPDATE OF title, body, tags ON notes BEGIN
    INSERT INTO notes_fts (notes_fts, rowid, title, body, tags) VALUES ('delete', old.id, old.title, old.body, old.tags);
    INSERT INTO notes_fts (rowid, title, body, tags) VALUES (new.id, new.title, new.body, new.tags);
END;

-- How many notes hold a word, for weighing query words against each other.
CREATE VIRTUAL TABLE notes_vocab USING fts5vocab (notes_fts, row);

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

-- Full state of a note after each change. Deleted together with the note.
CREATE TABLE note_revisions (
    note_id    INTEGER NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    revision   INTEGER NOT NULL,
    status     TEXT NOT NULL,
    title      TEXT NOT NULL,
    body       TEXT NOT NULL,
    tags       TEXT NOT NULL,
    parent_id  INTEGER,
    entity_id  INTEGER,
    confidence REAL,
    basis      TEXT,
    actor      TEXT,
    run        TEXT,
    origin     TEXT NOT NULL CHECK (origin IN ('recorded', 'legacy_baseline')),
    at         TEXT NOT NULL,
    PRIMARY KEY (note_id, revision)
) STRICT, WITHOUT ROWID;

-- An item is done or not; no state in between.
CREATE TABLE checklist_items (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    note_id    INTEGER NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    text       TEXT NOT NULL,
    done       INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0, 1)),
    done_by    TEXT,
    done_at    TEXT,
    created_at TEXT NOT NULL
) STRICT;

CREATE INDEX checklist_note ON checklist_items (note_id, id);

-- Append only. `fields` names what changed, never the values. task_id and
-- event_id are plain numbers on purpose: an entry outlives what it points at.
CREATE TABLE journal (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project     TEXT NOT NULL,
    record_type TEXT NOT NULL CHECK (record_type IN ('note', 'entity', 'checkpoint')),
    record_id   INTEGER NOT NULL,
    op          TEXT NOT NULL CHECK (op IN (
                    'baseline', 'created', 'updated', 'checked', 'claimed', 'released',
                    'linked', 'unlinked', 'redacted', 'purged')),
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

-- A checkpoint may belong to one task, and remembers where the journal stood.
ALTER TABLE checkpoints ADD COLUMN task_id INTEGER REFERENCES notes (id) ON DELETE CASCADE;
ALTER TABLE checkpoints ADD COLUMN run TEXT;
ALTER TABLE checkpoints ADD COLUMN journal_id INTEGER NOT NULL DEFAULT 0;

CREATE INDEX checkpoints_task ON checkpoints (task_id, id);

-- What was written before this version has no recorded history. Its state at
-- upgrade becomes revision 1, marked as a baseline rather than as a creation.
INSERT INTO note_revisions
    (note_id, revision, status, title, body, tags, parent_id, entity_id, actor, origin, at)
SELECT id, 1, status, title, body, tags, parent_id, entity_id, author, 'legacy_baseline', updated_at
FROM notes;

-- Entity events and checkpoints were recorded when they happened; they are
-- copied in, marked as backfilled.
INSERT INTO journal (project, record_type, record_id, op, revision, fields, actor, event_id, origin, at)
SELECT project, record_type, record_id, op, revision, '[]', actor, event_id, origin, at
FROM (
    SELECT project, 'note' AS record_type, id AS record_id, 'baseline' AS op,
           1 AS revision, author AS actor, NULL AS event_id, 'legacy_baseline' AS origin,
           updated_at AS at
    FROM notes
    UNION ALL
    SELECT e.project, 'entity', v.entity_id, v.event, NULL, v.author, v.id, 'backfilled', v.at
    FROM entity_events v JOIN entities e ON e.id = v.entity_id
    UNION ALL
    SELECT project, 'checkpoint', id, 'created', NULL, author, NULL, 'backfilled', created_at
    FROM checkpoints
)
ORDER BY at, record_type, record_id;

UPDATE checkpoints SET journal_id = (SELECT coalesce(max(id), 0) FROM journal);
