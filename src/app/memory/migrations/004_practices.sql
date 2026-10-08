-- Checklists, task summaries, confidence.

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

-- A task's summary is a note beneath it; at most one per task.
ALTER TABLE notes ADD COLUMN summary INTEGER NOT NULL DEFAULT 0 CHECK (summary IN (0, 1));

CREATE UNIQUE INDEX notes_summary ON notes (parent_id) WHERE summary = 1;

-- Kept beside the record, so confirming it makes no new revision.
CREATE TABLE assessments (
    record_type TEXT NOT NULL CHECK (record_type IN ('note', 'concept')),
    record_id   TEXT NOT NULL,
    confidence  REAL CHECK (confidence IS NULL OR (confidence >= 0 AND confidence <= 1)),
    basis       TEXT,
    verified_at TEXT,
    verified_by TEXT,
    PRIMARY KEY (record_type, record_id)
) STRICT, WITHOUT ROWID;

CREATE TRIGGER notes_assessment_delete AFTER DELETE ON notes BEGIN
    DELETE FROM assessments WHERE record_type = 'note' AND record_id = CAST(old.id AS TEXT);
END;

CREATE TRIGGER concepts_assessment_delete AFTER DELETE ON concepts BEGIN
    DELETE FROM assessments WHERE record_type = 'concept' AND record_id = old.id;
END;
