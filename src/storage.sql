-- 创建表格（如果不存在）
CREATE TABLE IF NOT EXISTS
    artifact (
        blake3 TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        src TEXT NOT NULL,
        len INTEGER NOT NULL,
        sha1 TEXT,
        sha256 TEXT,
        md5 TEXT
    );

CREATE INDEX IF NOT EXISTS idx_artifact_name ON artifact (name);

-- 为 sha1 创建索引（如果不存在）
CREATE UNIQUE INDEX IF NOT EXISTS idx_artifact_sha1 ON artifact (sha1);

-- 为 sha256 创建索引（如果不存在）
CREATE UNIQUE INDEX IF NOT EXISTS idx_artifact_sha256 ON artifact (sha256);

-- 为 md5 创建索引（如果不存在）
CREATE UNIQUE INDEX IF NOT EXISTS idx_artifact_md5 ON artifact (md5);
