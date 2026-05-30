CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    email TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    display_name TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    owner_id TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS project_members (
    project_id TEXT NOT NULL REFERENCES projects(id),
    user_id TEXT NOT NULL REFERENCES users(id),
    role TEXT NOT NULL DEFAULT 'editor',
    joined_at INTEGER NOT NULL,
    PRIMARY KEY (project_id, user_id)
);

CREATE TABLE IF NOT EXISTS invite_codes (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    code TEXT UNIQUE NOT NULL,
    max_uses INTEGER DEFAULT 10,
    used_count INTEGER DEFAULT 0,
    expires_at INTEGER,
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS files (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    parent_id TEXT REFERENCES files(id),
    name TEXT NOT NULL,
    type TEXT NOT NULL,
    mime_type TEXT,
    size INTEGER DEFAULT 0,
    storage_path TEXT,
    zone TEXT NOT NULL DEFAULT 'code',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id, parent_id, name)
);

CREATE TABLE IF NOT EXISTS file_blobs (
    file_id TEXT PRIMARY KEY REFERENCES files(id),
    content BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS crdt_docs (
    file_id TEXT PRIMARY KEY REFERENCES files(id),
    ydoc_state BLOB NOT NULL,
    updated_at INTEGER NOT NULL
);
