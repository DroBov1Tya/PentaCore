-- Task hierarchy: `uses` links, report pointers and live summaries with freshness.

-- edges is rebuilt, because a kind cannot be added to its CHECK in place.
-- This file runs with foreign keys off; the links are verified before the
-- migration commits (see db.rs).
CREATE TABLE edges_new (
    src        INTEGER NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    dst        INTEGER NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN (
                   'depends_on', 'supports', 'contradicts', 'answers', 'supersedes', 'relates_to',
                   'uses')),
    created_at TEXT NOT NULL,
    PRIMARY KEY (src, dst, kind),
    CHECK (src <> dst)
) STRICT, WITHOUT ROWID;

INSERT INTO edges_new (src, dst, kind, created_at)
SELECT src, dst, kind, created_at FROM edges;

DROP TABLE edges;
ALTER TABLE edges_new RENAME TO edges;

CREATE INDEX edges_dst ON edges (dst);

ALTER TABLE notes ADD COLUMN report_ids TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid(report_ids) AND json_type(report_ids) = 'array');
ALTER TABLE note_revisions ADD COLUMN report_ids TEXT NOT NULL DEFAULT '[]';

CREATE TABLE task_summary_state (
    task_id          INTEGER PRIMARY KEY REFERENCES notes (id) ON DELETE CASCADE,
    change_version   INTEGER NOT NULL DEFAULT 0,
    reviewed_version INTEGER,
    updated_at       TEXT NOT NULL,
    reviewed_at      TEXT
) STRICT;

-- Old summaries retain their text. Missing ones start empty, never presented as
-- reviewed. An agent must review both old and new summaries after the upgrade.
INSERT INTO notes (project, kind, status, title, body, tags, parent_id, summary,
                   created_at, updated_at)
SELECT n.project, 'fact', 'active', 'Summary: ' || substr(n.title, 1, 180), '', '', n.id, 1,
       strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM notes n WHERE n.kind IN ('goal', 'step')
    AND NOT EXISTS (SELECT 1 FROM notes s WHERE s.parent_id = n.id AND s.summary = 1);

INSERT INTO note_revisions (note_id, revision, status, title, body, tags, parent_id,
                            origin, at)
SELECT n.id, n.revision, n.status, n.title, n.body, n.tags, n.parent_id,
       'legacy_baseline', n.updated_at
FROM notes n WHERE n.summary = 1
    AND NOT EXISTS (SELECT 1 FROM note_revisions r WHERE r.note_id = n.id);

INSERT INTO journal (project, record_type, record_id, op, revision, fields, task_id, origin, at)
SELECT n.project, 'note', n.id, 'created', n.revision, '["summary"]', n.parent_id,
       'backfilled', n.updated_at
FROM notes n WHERE n.summary = 1
    AND NOT EXISTS (SELECT 1 FROM journal j WHERE j.record_type = 'note' AND j.record_id = n.id);

INSERT INTO task_summary_state (task_id, updated_at)
SELECT id, updated_at FROM notes WHERE kind IN ('goal', 'step');

-- Journal entries cover notes, checklists, edges, confirmations, entity events
-- and purges (recorded before deletion). Referenced reports affect their readers
-- even across branches. A summary update also invalidates its own reviewed mark;
-- summarize refreshes that mark only after the whole transaction succeeds.
CREATE TRIGGER summary_changed AFTER INSERT ON journal BEGIN
    UPDATE task_summary_state SET change_version = change_version + 1, updated_at = new.at
    WHERE task_id IN (
        WITH RECURSIVE affected (id) AS (
            SELECT n.id FROM notes n WHERE new.record_type = 'note' AND n.id = new.record_id
            UNION
            SELECT new.task_id WHERE new.task_id IS NOT NULL
            UNION
            SELECT n.id FROM notes n WHERE new.record_type = 'entity' AND n.entity_id = new.record_id
            UNION
            SELECT s.parent_id FROM notes s, json_each(s.report_ids) r
                WHERE s.summary = 1 AND new.record_type = 'note' AND r.value = new.record_id
            UNION
            SELECT n.parent_id FROM notes n JOIN affected a ON n.id = a.id
                WHERE n.parent_id IS NOT NULL
        ) SELECT id FROM affected
    );
END;

-- A moved note is no longer reachable from the old branch when its journal
-- entry is written, so invalidate the former ancestry before changing parent.
CREATE TRIGGER summary_moved BEFORE UPDATE OF parent_id ON notes
WHEN old.parent_id IS NOT new.parent_id BEGIN
    UPDATE task_summary_state SET change_version = change_version + 1,
        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE task_id IN (
        WITH RECURSIVE ancestors (id) AS (
            SELECT old.parent_id
            UNION
            SELECT n.parent_id FROM notes n JOIN ancestors a ON n.id = a.id
                WHERE n.parent_id IS NOT NULL
        ) SELECT id FROM ancestors
    );
END;
