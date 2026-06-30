-- Arena authorship tracking.
--
-- Adds two nullable columns to `files` so each Arena card records who
-- originally created it (`created_by`, immutable) and who most recently
-- edited it (`last_edited_by`, overwritten on every save / append_log).
--
-- Nullable on purpose: pre-existing Arena rows have no recorded author
-- and render as "Unknown" in the UI rather than be backfilled with a
-- guess. Existing handlers are expected to populate both columns going
-- forward (see server/src/arena/handlers.rs).

ALTER TABLE files ADD COLUMN created_by TEXT REFERENCES users(id);
ALTER TABLE files ADD COLUMN last_edited_by TEXT REFERENCES users(id);

-- Indexes for the "all cards by author X" / "all cards I recently
-- edited" filter views we plan to add in a later PR. Cheap on SQLite
-- for the data sizes here even with NULL-heavy distributions.
CREATE INDEX IF NOT EXISTS idx_files_arena_created_by
    ON files(project_id, created_by);
CREATE INDEX IF NOT EXISTS idx_files_arena_last_edited_by
    ON files(project_id, last_edited_by);