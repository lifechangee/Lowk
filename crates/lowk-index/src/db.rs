//! SQLite schema. The file is a rebuildable projection of the vault: deleting it loses nothing
//! (the app re-feeds items after `known()` comes back empty).
use lowk_types::{AppError, ErrorCode, FolderRef, Result, Ulid};
use rusqlite::Connection;
use std::path::Path;
use std::str::FromStr;

pub const SCHEMA_VER: i64 = 1;

pub fn open(path: &Path) -> Result<Connection> {
    let c = Connection::open(path).map_err(db_err)?;
    c.busy_timeout(std::time::Duration::from_secs(10))
        .map_err(db_err)?;
    c.pragma_update(None, "journal_mode", "WAL").map_err(db_err)?;
    c.pragma_update(None, "synchronous", "NORMAL").map_err(db_err)?;
    c.pragma_update(None, "temp_store", "MEMORY").map_err(db_err)?;
    c.pragma_update(None, "cache_size", -65536).map_err(db_err)?;
    Ok(c)
}

/// Creates or migrates the schema. An unknown (newer or corrupt) schema is dropped and
/// recreated: the index is only a cache of the vault.
pub fn migrate(c: &Connection) -> Result<()> {
    let ver: i64 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(db_err)?;
    if ver == SCHEMA_VER {
        return Ok(());
    }
    if ver != 0 {
        tracing::warn!(ver, "index schema mismatch; rebuilding the index file");
        for t in [
            "chunks_vocab",
            "chunks_fts",
            "vecs",
            "chunks",
            "file_tags",
            "files",
            "folders",
            "meta",
        ] {
            c.execute_batch(&format!("DROP TABLE IF EXISTS {t};"))
                .map_err(db_err)?;
        }
    }
    c.execute_batch(
        "
        CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE files(
            fid INTEGER PRIMARY KEY,
            id TEXT NOT NULL UNIQUE,
            kind TEXT NOT NULL,
            folder TEXT NOT NULL,
            title TEXT NOT NULL,
            tags TEXT NOT NULL DEFAULT '[]',
            when_ts TEXT,
            when_ms INTEGER,
            content_hash TEXT NOT NULL,
            pinned INTEGER NOT NULL DEFAULT 0,
            deleted INTEGER NOT NULL DEFAULT 0,
            chunker_ver INTEGER NOT NULL
        );
        CREATE INDEX files_folder ON files(folder);
        CREATE TABLE file_tags(fid INTEGER NOT NULL, tag TEXT NOT NULL, PRIMARY KEY(fid, tag)) WITHOUT ROWID;
        CREATE INDEX file_tags_tag ON file_tags(tag);
        CREATE TABLE chunks(
            id INTEGER PRIMARY KEY,
            cid TEXT NOT NULL UNIQUE,
            fid INTEGER NOT NULL,
            ord INTEGER NOT NULL,
            kind TEXT NOT NULL,
            start_s INTEGER,
            page INTEGER,
            heading TEXT NOT NULL DEFAULT '[]',
            text TEXT NOT NULL,
            n_tokens INTEGER NOT NULL
        );
        CREATE INDEX chunks_fid ON chunks(fid, ord);
        CREATE VIRTUAL TABLE chunks_fts USING fts5(stem, tokenize='unicode61 remove_diacritics 0');
        CREATE VIRTUAL TABLE chunks_vocab USING fts5vocab(chunks_fts, 'row');
        CREATE TABLE vecs(id INTEGER PRIMARY KEY, model TEXT NOT NULL, vec BLOB NOT NULL);
        CREATE TABLE folders(
            key TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            allow_cloud INTEGER NOT NULL,
            instructions TEXT NOT NULL,
            tags TEXT NOT NULL DEFAULT '[]'
        );
        ",
    )
    .map_err(db_err)?;
    c.pragma_update(None, "user_version", SCHEMA_VER)
        .map_err(db_err)?;
    Ok(())
}

pub fn db_err(e: rusqlite::Error) -> AppError {
    AppError::new(ErrorCode::Index).with_detail(e.to_string())
}

pub fn folder_key(f: &FolderRef) -> String {
    match f {
        FolderRef::User(u) => u.to_string(),
        FolderRef::Unsorted => lowk_types::SYS_UNSORTED_ID.to_string(),
        FolderRef::Trash => lowk_types::SYS_TRASH_ID.to_string(),
    }
}

pub fn parse_folder_key(k: &str) -> FolderRef {
    match k {
        lowk_types::SYS_UNSORTED_ID => FolderRef::Unsorted,
        lowk_types::SYS_TRASH_ID => FolderRef::Trash,
        _ => Ulid::from_str(k)
            .map(FolderRef::User)
            .unwrap_or(FolderRef::Unsorted),
    }
}

pub fn get_meta(c: &Connection, k: &str) -> Option<String> {
    c.query_row("SELECT value FROM meta WHERE key=?1", [k], |r| r.get(0))
        .ok()
}

pub fn set_meta(c: &Connection, k: &str, v: &str) -> Result<()> {
    c.execute(
        "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [k, v],
    )
    .map_err(db_err)?;
    Ok(())
}
