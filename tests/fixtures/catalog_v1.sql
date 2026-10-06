CREATE TABLE scans (
    id INTEGER PRIMARY KEY,
    root BLOB NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('running', 'complete', 'incomplete', 'interrupted')),
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    note TEXT,
    CHECK (state != 'running' OR finished_at IS NULL),
    CHECK (state NOT IN ('complete', 'incomplete') OR finished_at IS NOT NULL)
);
CREATE TABLE files (
    scan_id INTEGER NOT NULL REFERENCES scans(id),
    path BLOB NOT NULL,
    size TEXT NOT NULL CHECK (length(size) > 0 AND size NOT GLOB '*[^0-9]*'),
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
