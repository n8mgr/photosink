use crate::sources::Kind;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

pub struct State {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub(crate) struct Export {
    pub(crate) uuid: String,
    pub(crate) kind: Kind,
    pub(crate) path: String,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Placed {
    New(String),
    Duplicate(String),
}

impl State {
    fn from_conn(conn: Connection) -> Result<State> {
        conn.create_collation("photosink_path", |a, b| Path::new(a).cmp(Path::new(b)))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS assets (uuid TEXT PRIMARY KEY, dir TEXT NOT NULL, stem TEXT NOT NULL, UNIQUE (dir, stem));
             CREATE INDEX IF NOT EXISTS assets_path_stem ON assets (dir COLLATE photosink_path, stem);
             DROP INDEX IF EXISTS assets_stem;
             CREATE TABLE IF NOT EXISTS exports (uuid TEXT NOT NULL, kind TEXT NOT NULL, hash TEXT NOT NULL, path TEXT NOT NULL, PRIMARY KEY (uuid, kind));
             CREATE INDEX IF NOT EXISTS exports_hash ON exports (hash);
             CREATE INDEX IF NOT EXISTS exports_path ON exports (path COLLATE photosink_path);
             CREATE TABLE IF NOT EXISTS export_history (uuid TEXT NOT NULL, kind TEXT NOT NULL, hash TEXT NOT NULL, path TEXT NOT NULL, PRIMARY KEY (uuid, kind, path));
             CREATE INDEX IF NOT EXISTS export_history_hash ON export_history (hash);
             CREATE INDEX IF NOT EXISTS export_history_path ON export_history (path COLLATE photosink_path);
             CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        Ok(State {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }

    pub(crate) fn open(path: &Path) -> Result<State> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        State::from_conn(Connection::open(path)?)
    }

    pub(crate) fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row("SELECT value FROM kv WHERE key = ?", [key], |r| r.get(0))
            .optional()?)
    }

    pub(crate) fn set(&self, key: &str, value: &str) -> Result<()> {
        self.conn()
            .execute("INSERT OR REPLACE INTO kv VALUES (?, ?)", [key, value])?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn stem(&self, uuid: &str) -> Result<Option<String>> {
        Ok(self.location(uuid)?.map(|(_, stem)| stem))
    }

    /// The folder and stem the asset is placed under.
    pub(crate) fn location(&self, uuid: &str) -> Result<Option<(String, String)>> {
        Ok(self
            .conn()
            .query_row("SELECT dir, stem FROM assets WHERE uuid = ?", [uuid], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?)
    }

    pub(crate) fn claim_stem(&self, uuid: &str, dir: &str, stem: &str) -> Result<bool> {
        let conn = self.conn();
        if stem_owner(&conn, dir, stem)?.is_some() {
            return Ok(false);
        }
        Ok(conn.execute(
            "INSERT OR IGNORE INTO assets VALUES (?, ?, ?)",
            [uuid, dir, stem],
        )? == 1)
    }

    pub(crate) fn assets_under(&self, dir: &str) -> Result<Vec<(String, String, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT uuid, dir, stem FROM assets ORDER BY dir, stem")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        let mut assets = Vec::new();
        for row in rows {
            let asset: (String, String, String) = row?;
            if Path::new(&asset.1).starts_with(dir) {
                assets.push(asset);
            }
        }
        Ok(assets)
    }

    pub(crate) fn stem_owner(&self, dir: &str, stem: &str) -> Result<Option<String>> {
        stem_owner(&self.conn(), dir, stem)
    }

    pub(crate) fn exports_of(&self, uuid: &str) -> Result<Vec<(Kind, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT kind, path FROM exports WHERE uuid = ?")?;
        let rows = stmt.query_map([uuid], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub(crate) fn paths_of(&self, uuid: &str) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT path FROM exports WHERE uuid = ?1
             UNION SELECT path FROM export_history WHERE uuid = ?1",
        )?;
        let rows = stmt.query_map([uuid], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub(crate) fn stored_exports_of(&self, uuid: &str) -> Result<Vec<(Kind, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT kind, path FROM exports WHERE uuid = ?1
             UNION ALL SELECT kind, path FROM export_history WHERE uuid = ?1",
        )?;
        let rows = stmt.query_map([uuid], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub(crate) fn relocate(
        &self,
        uuid: &str,
        dir: &str,
        stem: &str,
        paths: &[(String, String)],
    ) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE assets SET dir = ?, stem = ? WHERE uuid = ?",
            [dir, stem, uuid],
        )?;
        tx.execute_batch(
            "CREATE TEMP TABLE relocation_paths (old TEXT PRIMARY KEY, new TEXT NOT NULL);",
        )?;
        for (old, path) in paths {
            tx.execute("INSERT INTO relocation_paths VALUES (?, ?)", [old, path])?;
        }
        // Content duplicates of this asset point at the same file.
        tx.execute_batch(
            "CREATE TEMP TABLE relocated_history AS
                 SELECT uuid, kind, hash, new AS path FROM export_history
                 JOIN relocation_paths ON path = old;
             UPDATE exports SET path = (SELECT new FROM relocation_paths WHERE old = path)
                 WHERE path IN (SELECT old FROM relocation_paths);
             DELETE FROM export_history WHERE path IN (SELECT old FROM relocation_paths);
             INSERT INTO export_history SELECT uuid, kind, hash, path FROM relocated_history;
             DROP TABLE relocated_history;
             DROP TABLE relocation_paths;",
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn shared(&self, path: &str) -> Result<bool> {
        let n: i64 = self.conn().query_row(
            "SELECT COUNT(DISTINCT uuid) FROM (
                SELECT uuid FROM exports WHERE path = ?1
                UNION SELECT uuid FROM export_history WHERE path = ?1
             )",
            [path],
            |r| r.get(0),
        )?;
        Ok(n > 1)
    }

    pub(crate) fn all_exports(&self) -> Result<Vec<Export>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT uuid, kind, path FROM exports
             UNION SELECT uuid, kind, path FROM export_history",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Export {
                uuid: r.get(0)?,
                kind: r.get(1)?,
                path: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub(crate) fn forget_export(&self, uuid: &str, kind: Kind, path: &str) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM exports WHERE uuid = ? AND kind = ? AND path = ?",
            params![uuid, kind, path],
        )?;
        tx.execute(
            "DELETE FROM export_history WHERE uuid = ? AND kind = ? AND path = ?",
            params![uuid, kind, path],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Drops the asset's folder claim and remaining rows; a later run places
    /// it afresh. The `taken:<uuid>` override, if any, is kept.
    pub(crate) fn forget_asset(&self, uuid: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM exports WHERE uuid = ?", [uuid])?;
        conn.execute("DELETE FROM export_history WHERE uuid = ?", [uuid])?;
        conn.execute("DELETE FROM assets WHERE uuid = ?", [uuid])?;
        Ok(())
    }

    pub(crate) fn exported(&self, uuid: &str, kind: Kind) -> Result<Option<String>> {
        exported(&self.conn(), uuid, kind)
    }

    pub(crate) fn revision(&self, uuid: &str, kind: Kind) -> Result<Option<String>> {
        self.get(&format!("revision:{uuid}:{}", kind.name()))
    }

    pub(crate) fn baseline_revision(&self, uuid: &str, kind: Kind, revision: &str) -> Result<()> {
        self.conn().execute(
            "INSERT OR IGNORE INTO kv VALUES (?, ?)",
            [
                format!("revision:{uuid}:{}", kind.name()),
                revision.to_owned(),
            ],
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn record(&self, uuid: &str, kind: Kind, hash: &str, path: &str) -> Result<()> {
        record(&self.conn(), uuid, kind, hash, path)
    }

    #[cfg(test)]
    pub(crate) fn duplicate(&self, hash: &str) -> Result<Option<String>> {
        duplicate(&self.conn(), hash)
    }

    /// Returns None when neither a duplicate nor a destination is available.
    pub(crate) fn place(
        &self,
        uuid: &str,
        kind: Kind,
        hash: &str,
        tmp: &Path,
        revision: Option<&str>,
        dest: impl FnOnce(&dyn Fn(&Path) -> Result<bool>) -> Result<Option<Placed>>,
    ) -> Result<Option<Placed>> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let placed = match duplicate(&tx, hash)? {
            Some(path) => {
                std::fs::remove_file(tmp)?;
                Placed::Duplicate(path)
            }
            None => match dest(&|path| {
                Ok(!tx.query_row(
                    "SELECT EXISTS (
                        SELECT 1 FROM exports WHERE path = ?1 COLLATE photosink_path AND hash != ?2
                        UNION ALL SELECT 1 FROM export_history WHERE path = ?1 COLLATE photosink_path AND hash != ?2
                     )",
                    params![path.to_string_lossy(), hash],
                    |r| r.get::<_, bool>(0),
                )?)
            })? {
                Some(placed) => placed,
                None => return Ok(None),
            },
        };
        let (Placed::New(path) | Placed::Duplicate(path)) = &placed;
        record(&tx, uuid, kind, hash, path)?;
        let key = format!("revision:{uuid}:{}", kind.name());
        if let Some(revision) = revision {
            tx.execute(
                "INSERT OR REPLACE INTO kv VALUES (?, ?)",
                [key, revision.to_owned()],
            )?;
        } else {
            tx.execute("DELETE FROM kv WHERE key = ?", [key])?;
        }
        tx.commit()?;
        Ok(Some(placed))
    }
}

fn stem_owner(conn: &Connection, dir: &str, stem: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT uuid FROM assets WHERE dir = ? COLLATE photosink_path AND stem = ?",
            [dir, stem],
            |r| r.get(0),
        )
        .optional()?)
}

fn exported(conn: &Connection, uuid: &str, kind: Kind) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT path FROM exports WHERE uuid = ? AND kind = ?",
            params![uuid, kind],
            |r| r.get(0),
        )
        .optional()?)
}

fn record(conn: &Connection, uuid: &str, kind: Kind, hash: &str, path: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO export_history
         SELECT uuid, kind, hash, path FROM exports WHERE uuid = ? AND kind = ? AND path != ?",
        params![uuid, kind, path],
    )?;
    conn.execute(
        "DELETE FROM export_history WHERE uuid = ? AND kind = ? AND path = ?",
        params![uuid, kind, path],
    )?;
    conn.execute(
        "INSERT OR REPLACE INTO exports VALUES (?, ?, ?, ?)",
        params![uuid, kind, hash, path],
    )?;
    Ok(())
}

fn duplicate(conn: &Connection, hash: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT path FROM exports WHERE hash = ?1
         UNION ALL SELECT path FROM export_history WHERE hash = ?1",
    )?;
    for path in stmt.query_map([hash], |r| r.get::<_, String>(0))? {
        let path = path?;
        if Path::new(&path).is_file() {
            return Ok(Some(path));
        }
    }
    Ok(None)
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
            Ok(Some(Placed::New(to.to_string_lossy().into_owned())))
        };
        assert_eq!(s.exported("u", Kind::Original).unwrap(), None);
        assert_eq!(s.duplicate("h").unwrap(), None);
        std::fs::write(&tmp, b"a").unwrap();
        assert_eq!(
            s.place("u", Kind::Original, "h", &tmp, None, |_| mv(&dest))
                .unwrap(),
            Some(Placed::New(dest.to_string_lossy().into_owned()))
        );
        assert!(dest.exists() && !tmp.exists());
        assert_eq!(
            s.exported("u", Kind::Original).unwrap().as_deref(),
            Some(dest.to_str().unwrap())
        );
        assert_eq!(s.exported("u", Kind::Edited).unwrap(), None);
        assert_eq!(s.duplicate("h").unwrap().as_deref(), dest.to_str());
        std::fs::write(&tmp, b"a").unwrap();
        assert_eq!(
            s.place("v", Kind::Original, "h", &tmp, None, |_| unreachable!())
                .unwrap(),
            Some(Placed::Duplicate(dest.to_string_lossy().into_owned()))
        );
        assert!(!tmp.exists());
        assert!(s.exported("v", Kind::Original).unwrap().is_some());
    }

    #[test]
    fn revisions_follow_successfully_placed_files() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::from_conn(Connection::open_in_memory().unwrap()).unwrap();
        let tmp = dir.path().join("part");
        let old = dir.path().join("old.jpg");
        let new = dir.path().join("new.jpg");
        std::fs::write(&tmp, b"old").unwrap();
        state
            .place("u", Kind::Original, "old", &tmp, Some("1"), |_| {
                std::fs::rename(&tmp, &old)?;
                Ok(Some(Placed::New(old.to_string_lossy().into_owned())))
            })
            .unwrap();
        std::fs::write(&tmp, b"new").unwrap();
        assert!(
            state
                .place("u", Kind::Original, "new", &tmp, Some("2"), |_| {
                    anyhow::bail!("placement failed")
                })
                .is_err()
        );
        assert_eq!(
            state.revision("u", Kind::Original).unwrap().as_deref(),
            Some("1")
        );
        assert_eq!(
            state.exported("u", Kind::Original).unwrap().as_deref(),
            old.to_str()
        );
        state
            .place("u", Kind::Original, "new", &tmp, None, |_| {
                std::fs::rename(&tmp, &new)?;
                Ok(Some(Placed::New(new.to_string_lossy().into_owned())))
            })
            .unwrap();
        assert_eq!(state.revision("u", Kind::Original).unwrap(), None);
        assert_eq!(
            state.exported("u", Kind::Original).unwrap().as_deref(),
            new.to_str()
        );
        assert_eq!(std::fs::read(old).unwrap(), b"old");
    }

    #[test]
    fn rechecks_duplicates_after_preparing_a_destination() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::open(&dir.path().join("state.sqlite")).unwrap();
        let tmp = dir.path().join("part");
        std::fs::write(&tmp, b"old").unwrap();
        assert_eq!(
            state
                .place("u", Kind::Original, "old", &tmp, Some("1"), |_| Ok(None))
                .unwrap(),
            None
        );
        assert!(tmp.exists());
        assert_eq!(state.revision("u", Kind::Original).unwrap(), None);
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let old = dir.path().join("old.jpg");
                    let new = dir.path().join("new.jpg");
                    std::fs::write(&old, b"old").unwrap();
                    std::fs::write(&new, b"new").unwrap();
                    state
                        .record("v", Kind::Original, "old", old.to_str().unwrap())
                        .unwrap();
                    state
                        .record("v", Kind::Original, "new", new.to_str().unwrap())
                        .unwrap();
                })
                .join()
                .unwrap();
        });
        let old = dir.path().join("old.jpg").to_string_lossy().into_owned();
        assert_eq!(
            state
                .place(
                    "u",
                    Kind::Original,
                    "old",
                    &tmp,
                    Some("1"),
                    |_| unreachable!()
                )
                .unwrap(),
            Some(Placed::Duplicate(old.clone()))
        );
        assert!(!tmp.exists());
        assert_eq!(
            state.exported("u", Kind::Original).unwrap(),
            Some(old.clone())
        );
        assert_eq!(
            state.revision("u", Kind::Original).unwrap().as_deref(),
            Some("1")
        );
        assert!(state.shared(&old).unwrap());
    }

    #[test]
    fn relocates_assets() {
        let s = State::from_conn(Connection::open_in_memory().unwrap()).unwrap();
        s.claim_stem("u", "/e/a", "IMG_1").unwrap();
        s.claim_stem("v", "/e/a/sub", "IMG_2").unwrap();
        s.claim_stem("w", "/e/ab", "IMG_3").unwrap();
        s.record("u", Kind::Original, "h1", "/e/a/IMG_1.jpg")
            .unwrap();
        s.record("u", Kind::Edited, "h2", "/e/a/IMG_1_edited.jpg")
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
            &[("/e/a/IMG_1.jpg".into(), "/e/b/IMG_1_x.jpg".into())],
        )
        .unwrap();
        assert_eq!(s.stem("u").unwrap().as_deref(), Some("IMG_1_x"));
        let mut ex = s.exports_of("u").unwrap();
        ex.sort();
        assert_eq!(
            ex,
            [
                (Kind::Original, "/e/b/IMG_1_x.jpg".to_string()),
                (Kind::Edited, "/e/a/IMG_1_edited.jpg".to_string())
            ]
        );
        assert!(s.assets_under("/e/a").unwrap().iter().all(|a| a.0 == "v"));
    }

    #[test]
    fn finds_assets_by_path_components() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("out_%");
        let state = State::from_conn(Connection::open_in_memory().unwrap()).unwrap();
        for (uuid, path) in [
            ("root", dir.clone()),
            ("child", dir.join("child")),
            ("nested", dir.join("child/nested")),
            ("sibling", root.path().join("out_%-other")),
        ] {
            state
                .claim_stem(uuid, &path.to_string_lossy(), "IMG")
                .unwrap();
        }
        for query in [dir.clone(), dir.join("")] {
            let mut ids: Vec<_> = state
                .assets_under(&query.to_string_lossy())
                .unwrap()
                .into_iter()
                .map(|(uuid, _, _)| uuid)
                .collect();
            ids.sort();
            assert_eq!(ids, ["child", "nested", "root"]);
        }
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

    #[test]
    fn stems_match_equivalent_directories() {
        let mut dirs = vec!["/e/a", "/e/./a", "/e//a/"];
        if cfg!(windows) {
            dirs.extend([r"\e\a", r"\e/a"]);
        }
        for stored in &dirs {
            let s = State::from_conn(Connection::open_in_memory().unwrap()).unwrap();
            s.conn()
                .execute(
                    "INSERT INTO assets VALUES (?, ?, ?)",
                    ["u", stored, "IMG_1"],
                )
                .unwrap();
            for query in &dirs {
                assert_eq!(s.stem_owner(query, "IMG_1").unwrap().as_deref(), Some("u"));
                assert!(!s.claim_stem("v", query, "IMG_1").unwrap());
            }
            assert_eq!(
                s.location("u").unwrap(),
                Some(((*stored).into(), "IMG_1".into()))
            );
            assert!(s.claim_stem("v", "/e/b", "IMG_1").unwrap());
            assert!(s.claim_stem("w", stored, "IMG_2").unwrap());
            if !cfg!(windows) {
                assert!(s.claim_stem("x", r"/e\a", "IMG_1").unwrap());
            }
        }
    }

    #[test]
    fn repeated_stems_use_indexed_directory_lookups() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let s = State::from_conn(Connection::open_in_memory().unwrap()).unwrap();
        let comparisons = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&comparisons);
        s.conn()
            .create_collation("photosink_path", move |a, b| {
                count.fetch_add(1, Ordering::Relaxed);
                Path::new(a).cmp(Path::new(b))
            })
            .unwrap();
        for i in 0..2_000 {
            assert!(
                s.claim_stem(&i.to_string(), &format!("/export/{i}"), "image")
                    .unwrap()
            );
        }
        comparisons.store(0, Ordering::Relaxed);
        assert_eq!(
            s.stem_owner("/export/./1999", "image").unwrap().as_deref(),
            Some("1999")
        );
        assert_eq!(s.stem_owner("/export/missing", "image").unwrap(), None);
        let count = comparisons.load(Ordering::Relaxed);
        assert!(count > 0 && count < 100, "{count} path comparisons");
    }

    #[test]
    fn indexes_existing_claims_and_reopens() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state.sqlite");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE assets (uuid TEXT PRIMARY KEY, dir TEXT NOT NULL, stem TEXT NOT NULL, UNIQUE (dir, stem));
             CREATE INDEX assets_stem ON assets (stem);
             INSERT INTO assets VALUES ('u', '/e/./a', 'IMG_1');",
        )
        .unwrap();
        drop(conn);
        for _ in 0..2 {
            let s = State::open(&path).unwrap();
            assert_eq!(s.stem_owner("/e/a", "IMG_1").unwrap().as_deref(), Some("u"));
            assert!(!s.claim_stem("v", "/e/a", "IMG_1").unwrap());
            assert_eq!(
                s.location("u").unwrap(),
                Some(("/e/./a".into(), "IMG_1".into()))
            );
        }
    }
}
