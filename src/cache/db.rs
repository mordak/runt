use crate::cache::messagemeta::MessageMeta;
use rusqlite::{Connection, params};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub struct Db {
    dbpath: PathBuf,
}

impl Db {
    fn init_db(path: &Path) -> Result<(), String> {
        let conn = Connection::open(path)
            .map_err(|e| format!("DB Open failed at {}: {}", path.display(), e))?;

        conn.execute(
            "CREATE TABLE v1 (
                uid                     INTEGER PRIMARY KEY,
                size                    INTEGER,
                internal_date_millis    INTEGER,
                flags                   TEXT,
                id                      TEXT
            )",
            params![],
        )
        .map(|_| ())
        .map_err(|e| format!("CREATE TABLE: {}", e))
    }

    pub fn from_file(path: &Path) -> Result<Db, String> {
        if !path.exists() {
            Db::init_db(path)?;
        }
        Ok(Db {
            dbpath: path.to_path_buf(),
        })
    }

    pub fn add(&self, meta: &MessageMeta) -> Result<(), String> {
        Connection::open(&self.dbpath)
            .and_then(|conn| {
                conn.execute(
                    "INSERT INTO v1 (uid, size, internal_date_millis, flags, id)
                                VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        meta.uid(),
                        meta.size(),
                        meta.internal_date_millis(),
                        meta.flags(),
                        meta.id()
                    ],
                )
            })
            .map(|_| ())
            .map_err(|e| format!("INSERT FAILED: {}", e))
    }

    pub fn update(&self, meta: &MessageMeta) -> Result<(), String> {
        Connection::open(&self.dbpath)
            .and_then(|conn| {
                conn.execute(
                    "UPDATE v1 SET uid = (?1),
                                   size = (?2),
                                   internal_date_millis = (?3),
                                   flags = (?4),
                                   id = (?5)
                                WHERE uid = (?1)",
                    params![
                        meta.uid(),
                        meta.size(),
                        meta.internal_date_millis(),
                        meta.flags(),
                        meta.id()
                    ],
                )
            })
            .map(|_| ())
            .map_err(|e| format!("UPDATE FAILED: {}", e))
    }

    pub fn delete_uid(&self, uid: u32) -> Result<(), String> {
        Connection::open(&self.dbpath)
            .and_then(|conn| conn.execute("DELETE from v1 WHERE uid = (?1)", params![uid]))
            .map(|_| ())
            .map_err(|e| format!("DELETE FAILED {}: {}", uid, e))
    }

    pub fn num_entries(&self) -> Result<i64, String> {
        let conn = Connection::open(&self.dbpath).map_err(|e| format!("Open DB: {}", e))?;
        let mut stmt = conn
            .prepare("SELECT count(uid) from v1")
            .map_err(|e| format!("SELECT: {}", e))?;

        stmt.query_row(params![], |r| Ok(r.get_unwrap(0)))
            .map_err(|e| format!("query_row: {}", e))
    }

    pub fn expected_entries(&self) -> usize {
        if let Ok(n) = self.num_entries() {
            n as usize
        } else {
            100
        }
    }

    pub fn get_uids(&self) -> Result<HashSet<u32>, String> {
        let mut v = HashSet::with_capacity(self.expected_entries());
        let conn = Connection::open(&self.dbpath).map_err(|e| format!("Open DB: {}", e))?;

        let mut stmt = conn
            .prepare("SELECT uid FROM v1")
            .map_err(|e| format!("SELECT FAILED: {}", e))?;

        let rows = stmt
            .query_map(params![], |r| r.get(0))
            .map_err(|e| format!("query_map: {}", e))?;

        for r in rows {
            v.insert(r.map_err(|e| format!("fetch row: {}", e))?);
        }
        Ok(v)
    }

    pub fn get_ids(&self) -> Result<HashMap<String, MessageMeta>, String> {
        let conn = Connection::open(&self.dbpath).map_err(|e| format!("Open DB: {}", e))?;

        let mut stmt = conn
            .prepare("SELECT uid, size, internal_date_millis, flags, id FROM v1")
            .map_err(|e| format!("SELECT FAILED: {}", e))?;

        let mut h = HashMap::with_capacity(self.expected_entries());
        let rows = stmt
            .query_map(params![], |r| {
                Ok(MessageMeta::from_fields(
                    r.get_unwrap(0),
                    r.get_unwrap(1),
                    r.get_unwrap(2),
                    r.get_unwrap(3),
                    r.get_unwrap(4),
                ))
            })
            .map_err(|e| format!("query_map: {}", e))?;

        for meta in rows.flatten() {
            h.insert(meta.id().to_string(), meta);
        }
        Ok(h)
    }

    pub fn get_uid(&self, uid: u32) -> anyhow::Result<MessageMeta> {
        let conn = Connection::open(&self.dbpath)?;

        let mut stmt = conn.prepare(
            "SELECT uid, size, internal_date_millis, flags, id
                      FROM v1 WHERE uid = (?)",
        )?;

        let res = stmt.query_row(params![uid], |r| {
            Ok(MessageMeta::from_fields(
                r.get_unwrap(0),
                r.get_unwrap(1),
                r.get_unwrap(2),
                r.get_unwrap(3),
                r.get_unwrap(4),
            ))
        })?;
        Ok(res)
    }

    pub fn get_id(&self, id: &str) -> Result<MessageMeta, String> {
        let conn = Connection::open(&self.dbpath).map_err(|e| format!("Open DB: {}", e))?;

        let mut stmt = conn
            .prepare(
                "SELECT uid, size, internal_date_millis, flags, id
                      FROM v1 WHERE id = (?)",
            )
            .map_err(|e| format!("SELECT: {}", e))?;

        stmt.query_row(params![id], |r| {
            Ok(MessageMeta::from_fields(
                r.get_unwrap(0),
                r.get_unwrap(1),
                r.get_unwrap(2),
                r.get_unwrap(3),
                r.get_unwrap(4),
            ))
        })
        .map_err(|e| format!("query_row: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_round_trip() {
        let dir = std::env::temp_dir().join(format!("runt-db-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cache.db");
        let db = Db::from_file(&path).unwrap();
        assert_eq!(db.num_entries().unwrap(), 0);

        let meta = MessageMeta::from_fields(7, 1234, 1_600_000_000_000, "FS".into(), "a.b".into());
        db.add(&meta).unwrap();
        db.add(&MessageMeta::from_fields(9, 1, 2, "".into(), "c.d".into()))
            .unwrap();
        assert_eq!(db.num_entries().unwrap(), 2);
        assert_eq!(db.get_uids().unwrap(), HashSet::from([7, 9]));

        let got = db.get_uid(7).unwrap();
        assert_eq!(got.id(), "a.b");
        assert_eq!(got.size(), 1234);
        assert_eq!(got.internal_date_millis(), 1_600_000_000_000);
        assert_eq!(got.flags(), meta.flags());
        assert_eq!(db.get_id("c.d").unwrap().uid(), 9);
        assert!(db.get_ids().unwrap().contains_key("a.b"));

        db.update(&MessageMeta::from_fields(
            7,
            1234,
            1_600_000_000_000,
            "S".into(),
            "a.b".into(),
        ))
        .unwrap();
        assert_eq!(db.get_uid(7).unwrap().flags(), "S");

        // SyncDir relies on a missing UID surfacing as QueryReturnedNoRows.
        db.delete_uid(7).unwrap();
        let err = db.get_uid(7).unwrap_err();
        assert!(matches!(
            err.downcast_ref::<rusqlite::Error>(),
            Some(rusqlite::Error::QueryReturnedNoRows)
        ));

        std::fs::remove_dir_all(&dir).ok();
    }
}
