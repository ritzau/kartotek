ALTER TABLE scans ADD COLUMN hashing INTEGER NOT NULL DEFAULT 0 CHECK (hashing IN (0, 1));
ALTER TABLE files ADD COLUMN device TEXT;
ALTER TABLE files ADD COLUMN inode TEXT;
ALTER TABLE files ADD COLUMN links TEXT;
ALTER TABLE files ADD COLUMN modified_seconds INTEGER;
ALTER TABLE files ADD COLUMN modified_nanos INTEGER;
ALTER TABLE files ADD COLUMN changed_seconds INTEGER;
ALTER TABLE files ADD COLUMN changed_nanos INTEGER;
ALTER TABLE files ADD COLUMN mode INTEGER;
ALTER TABLE files ADD COLUMN uid INTEGER;
ALTER TABLE files ADD COLUMN gid INTEGER;
ALTER TABLE files ADD COLUMN prefix_hash BLOB CHECK (prefix_hash IS NULL OR length(prefix_hash) = 32);
ALTER TABLE files ADD COLUMN full_hash BLOB CHECK (full_hash IS NULL OR length(full_hash) = 32);
ALTER TABLE files ADD COLUMN hash_state TEXT NOT NULL DEFAULT 'not_requested'
    CHECK (hash_state IN ('not_requested', 'pending', 'prefix', 'full', 'failed'))
    CHECK ((hash_state IN ('not_requested', 'pending') AND prefix_hash IS NULL AND full_hash IS NULL)
        OR (hash_state = 'failed' AND full_hash IS NULL)
        OR (hash_state = 'prefix' AND prefix_hash IS NOT NULL AND full_hash IS NULL)
        OR (hash_state = 'full' AND prefix_hash IS NOT NULL AND full_hash IS NOT NULL));
CREATE INDEX files_by_prefix ON files (scan_id, size, prefix_hash);
CREATE INDEX files_by_full_hash ON files (scan_id, size, full_hash);
