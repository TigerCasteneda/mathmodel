CREATE TABLE IF NOT EXISTS snapshots (
    id TEXT PRIMARY KEY,
    file_id TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    label TEXT,
    ydoc_state BLOB NOT NULL,
    created_by TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'manual',
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_snapshots_file_created ON snapshots(file_id, created_at);
CREATE INDEX IF NOT EXISTS idx_snapshots_project_created ON snapshots(project_id, created_at);
