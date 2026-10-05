CREATE TABLE scans (
    id INTEGER PRIMARY KEY,
    root BLOB NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('running', 'complete', 'incomplete', 'interrupted')),
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    note TEXT,
    hashing INTEGER NOT NULL DEFAULT 0 CHECK (hashing IN (0, 1)),
    CHECK (state != 'running' OR finished_at IS NULL),
    CHECK (state NOT IN ('complete', 'incomplete') OR finished_at IS NOT NULL)
);
CREATE TABLE files (
    scan_id INTEGER NOT NULL REFERENCES scans(id),
    path BLOB NOT NULL,
    size TEXT NOT NULL CHECK (length(size) > 0 AND size NOT GLOB '*[^0-9]*'),
    device TEXT,
    inode TEXT,
    links TEXT,
    modified_seconds INTEGER,
    modified_nanos INTEGER,
    changed_seconds INTEGER,
    changed_nanos INTEGER,
    mode INTEGER,
    uid INTEGER,
    gid INTEGER,
    prefix_hash BLOB CHECK (prefix_hash IS NULL OR length(prefix_hash) = 32),
    full_hash BLOB CHECK (full_hash IS NULL OR length(full_hash) = 32),
    hash_state TEXT NOT NULL DEFAULT 'not_requested'
        CHECK (hash_state IN ('not_requested', 'pending', 'prefix', 'full', 'failed'))
        CHECK ((hash_state IN ('not_requested', 'pending') AND prefix_hash IS NULL AND full_hash IS NULL)
            OR (hash_state = 'failed' AND full_hash IS NULL)
            OR (hash_state = 'prefix' AND prefix_hash IS NOT NULL AND full_hash IS NULL)
            OR (hash_state = 'full' AND prefix_hash IS NOT NULL AND full_hash IS NOT NULL)),
    PRIMARY KEY (scan_id, path)
);
CREATE TABLE read_failures (
    scan_id INTEGER NOT NULL REFERENCES scans(id),
    path BLOB NOT NULL,
    kind TEXT NOT NULL,
    message TEXT NOT NULL
);
CREATE INDEX failures_by_scan ON read_failures (scan_id);
CREATE UNIQUE INDEX one_running_scan ON scans (state) WHERE state = 'running';
CREATE INDEX files_by_prefix ON files (scan_id, size, prefix_hash);
CREATE INDEX files_by_full_hash ON files (scan_id, size, full_hash);
