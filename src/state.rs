use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::sync::Mutex;

pub struct State {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct Export {
    pub uuid: String,
    pub kind: String,
    pub hash: String,
    pub path: String,
}

#[derive(Debug, PartialEq)]
pub enum Placed {
    New(String),
    Duplicate(String),
}

impl State {
    pub fn open(path: &Path) -> Result<State> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        State::from_conn(Connection::open(path)?)
    }

    fn from_conn(conn: Connection) -> Result<State> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS assets (uuid TEXT PRIMARY KEY, dir TEXT NOT NULL, stem TEXT NOT NULL, UNIQUE (dir, stem));
             CREATE TABLE IF NOT EXISTS exports (uuid TEXT NOT NULL, kind TEXT NOT NULL, hash TEXT NOT NULL, path TEXT NOT NULL, PRIMARY KEY (uuid, kind));
             CREATE INDEX IF NOT EXISTS exports_hash ON exports (hash);
             CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        Ok(State {
            conn: Mutex::new(conn),
        })
    }

    pub fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT value FROM kv WHERE key = ?", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("INSERT OR REPLACE INTO kv VALUES (?, ?)", [key, value])?;
        Ok(())
    }

    pub fn stem(&self, uuid: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT stem FROM assets WHERE uuid = ?", [uuid], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn location(&self, uuid: &str) -> Result<Option<(String, String)>> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT dir, stem FROM assets WHERE uuid = ?", [uuid], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?)
    }

    pub fn claim_stem(&self, uuid: &str, dir: &str, stem: &str) -> Result<bool> {
        Ok(self.conn.lock().unwrap().execute(
            "INSERT OR IGNORE INTO assets VALUES (?, ?, ?)",
            [uuid, dir, stem],
        )? == 1)
    }

    pub fn assets_under(&self, dir: &str) -> Result<Vec<(String, String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT uuid, dir, stem FROM assets WHERE dir = ?1 OR dir LIKE ?2 ESCAPE '\\' ORDER BY dir, stem",
        )?;
        let like = format!(
            "{}/%",
            dir.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let rows = stmt.query_map([dir, &like], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn stem_owner(&self, dir: &str, stem: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT uuid FROM assets WHERE dir = ? AND stem = ?",
                [dir, stem],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn exports_of(&self, uuid: &str) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT kind, path FROM exports WHERE uuid = ?")?;
        let rows = stmt.query_map([uuid], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn relocate(
        &self,
        uuid: &str,
        dir: &str,
        stem: &str,
        paths: &[(String, String)],
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE assets SET dir = ?, stem = ? WHERE uuid = ?",
            [dir, stem, uuid],
        )?;
        for (kind, path) in paths {
            let old: Option<String> = tx
                .query_row(
                    "SELECT path FROM exports WHERE uuid = ? AND kind = ?",
                    [uuid, kind],
                    |r| r.get(0),
                )
                .optional()?;
            tx.execute(
                "UPDATE exports SET path = ? WHERE uuid = ? AND kind = ?",
                [path, uuid, kind],
            )?;
            // Content duplicates of this asset point at the same file.
            if let Some(old) = old {
                tx.execute("UPDATE exports SET path = ? WHERE path = ?", [path, &old])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn shared(&self, path: &str) -> Result<bool> {
        let n: i64 = self.conn.lock().unwrap().query_row(
            "SELECT COUNT(DISTINCT uuid) FROM exports WHERE path = ?",
            [path],
            |r| r.get(0),
        )?;
        Ok(n > 1)
    }

    pub fn all_exports(&self) -> Result<Vec<Export>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT uuid, kind, hash, path FROM exports ORDER BY rowid")?;
        let rows = stmt.query_map([], |r| {
            Ok(Export {
                uuid: r.get(0)?,
                kind: r.get(1)?,
                hash: r.get(2)?,
                path: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn forget_export(&self, uuid: &str, kind: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "DELETE FROM exports WHERE uuid = ? AND kind = ?",
            [uuid, kind],
        )?;
        Ok(())
    }

    /// Drops the asset's folder claim and remaining rows; a later run places
    /// it afresh. The `taken:<uuid>` override, if any, is kept.
    pub fn forget_asset(&self, uuid: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM exports WHERE uuid = ?", [uuid])?;
        conn.execute("DELETE FROM assets WHERE uuid = ?", [uuid])?;
        Ok(())
    }

    pub fn exported(&self, uuid: &str, kind: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT path FROM exports WHERE uuid = ? AND kind = ?",
                [uuid, kind],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn record(&self, uuid: &str, kind: &str, hash: &str, path: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT OR REPLACE INTO exports VALUES (?, ?, ?, ?)",
            params![uuid, kind, hash, path],
        )?;
        Ok(())
    }

    pub fn duplicate(&self, hash: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT path FROM exports WHERE hash = ? LIMIT 1",
                [hash],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn place(
        &self,
        uuid: &str,
        kind: &str,
        hash: &str,
        tmp: &Path,
        dest: impl FnOnce() -> Result<Placed>,
    ) -> Result<Placed> {
        let conn = self.conn.lock().unwrap();
        let existing: Option<String> = conn
            .query_row(
                "SELECT path FROM exports WHERE hash = ? LIMIT 1",
                [hash],
                |r| r.get(0),
            )
            .optional()?;
        let placed = match existing {
            Some(path) => {
                std::fs::remove_file(tmp)?;
                Placed::Duplicate(path)
            }
            None => dest()?,
        };
        let (Placed::New(path) | Placed::Duplicate(path)) = &placed;
        conn.execute(
            "INSERT OR REPLACE INTO exports VALUES (?, ?, ?, ?)",
            params![uuid, kind, hash, path],
        )?;
        Ok(placed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kv_roundtrip() {
        let state = State::from_conn(Connection::open_in_memory().unwrap()).unwrap();
        assert_eq!(state.get("k").unwrap(), None);
        state.set("k", "a").unwrap();
        state.set("k", "b").unwrap();
        assert_eq!(state.get("k").unwrap().as_deref(), Some("b"));
    }

    #[test]
    fn places_and_dedups() {
        let dir = tempfile::tempdir().unwrap();
        let s = State::from_conn(Connection::open_in_memory().unwrap()).unwrap();
        let tmp = dir.path().join(".part");
        let dest = dir.path().join("IMG_1.HEIC");
        let mv = |to: &Path| {
            std::fs::rename(&tmp, to)?;
            Ok(Placed::New(to.to_string_lossy().into_owned()))
        };
        assert_eq!(s.exported("u", "original").unwrap(), None);
        assert_eq!(s.duplicate("h").unwrap(), None);
        std::fs::write(&tmp, b"a").unwrap();
        assert_eq!(
            s.place("u", "original", "h", &tmp, || mv(&dest)).unwrap(),
            Placed::New(dest.to_string_lossy().into_owned())
        );
        assert!(dest.exists() && !tmp.exists());
        assert_eq!(
            s.exported("u", "original").unwrap().as_deref(),
            Some(dest.to_str().unwrap())
        );
        assert_eq!(s.exported("u", "edited").unwrap(), None);
        assert_eq!(s.duplicate("h").unwrap().as_deref(), dest.to_str());
        std::fs::write(&tmp, b"a").unwrap();
        assert_eq!(
            s.place("v", "original", "h", &tmp, || unreachable!())
                .unwrap(),
            Placed::Duplicate(dest.to_string_lossy().into_owned())
        );
        assert!(!tmp.exists());
        assert!(s.exported("v", "original").unwrap().is_some());
    }

    #[test]
    fn relocates_assets() {
        let s = State::from_conn(Connection::open_in_memory().unwrap()).unwrap();
        s.claim_stem("u", "/e/a", "IMG_1").unwrap();
        s.claim_stem("v", "/e/a/sub", "IMG_2").unwrap();
        s.claim_stem("w", "/e/ab", "IMG_3").unwrap();
        s.record("u", "original", "h1", "/e/a/IMG_1.jpg").unwrap();
        s.record("u", "edited", "h2", "/e/a/IMG_1_edited.jpg")
            .unwrap();
        let under = s.assets_under("/e/a").unwrap();
        assert_eq!(under.len(), 2);
        assert_eq!(under[0].0, "u");
        assert_eq!(s.stem_owner("/e/a", "IMG_1").unwrap().as_deref(), Some("u"));
        assert_eq!(s.stem_owner("/e/b", "IMG_1").unwrap(), None);
        s.relocate(
            "u",
            "/e/b",
            "IMG_1_x",
            &[("original".into(), "/e/b/IMG_1_x.jpg".into())],
        )
        .unwrap();
        assert_eq!(s.stem("u").unwrap().as_deref(), Some("IMG_1_x"));
        let mut ex = s.exports_of("u").unwrap();
        ex.sort();
        assert_eq!(
            ex,
            [
                ("edited".to_string(), "/e/a/IMG_1_edited.jpg".to_string()),
                ("original".to_string(), "/e/b/IMG_1_x.jpg".to_string())
            ]
        );
        assert!(s.assets_under("/e/a").unwrap().iter().all(|a| a.0 == "v"));
    }

    #[test]
    fn stems_are_unique_per_dir() {
        let s = State::from_conn(Connection::open_in_memory().unwrap()).unwrap();
        assert_eq!(s.stem("u").unwrap(), None);
        assert!(s.claim_stem("u", "/d", "IMG_1").unwrap());
        assert!(!s.claim_stem("v", "/d", "IMG_1").unwrap());
        assert!(s.claim_stem("v", "/e", "IMG_1").unwrap());
        assert_eq!(s.stem("u").unwrap().as_deref(), Some("IMG_1"));
        assert_eq!(s.stem("v").unwrap().as_deref(), Some("IMG_1"));
    }
}
