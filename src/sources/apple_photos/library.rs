use crate::photo::{Asset, Camera};
use crate::{Event, Progress};
use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::path::Path;

fn cocoa_time(secs: f64, offset: i32) -> Option<DateTime<FixedOffset>> {
    let utc = Utc.timestamp_opt(978_307_200 + secs as i64, 0).single()?;
    Some(utc.with_timezone(&FixedOffset::east_opt(offset)?))
}

fn find_join(
    conn: &Connection,
    a_suffix: &str,
    b_suffix: &str,
) -> Result<Option<(String, String, String)>> {
    let mut tables = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'Z\\_%' ESCAPE '\\'",
    )?;
    for table in tables.query_map([], |r| r.get::<_, String>(0))? {
        let table = table?;
        let mut cols = conn.prepare(&format!("PRAGMA table_info(\"{table}\")"))?;
        let cols: Vec<String> = cols
            .query_map([], |r| r.get(1))?
            .collect::<Result<_, _>>()?;
        let find = |suffix: &str| {
            cols.iter()
                .find(|c| c.starts_with("Z_") && !c.starts_with("Z_FOK") && c.ends_with(suffix))
                .cloned()
        };
        if let (Some(a), Some(b)) = (find(a_suffix), find(b_suffix)) {
            return Ok(Some((table, a, b)));
        }
    }
    Ok(None)
}

fn grouped(conn: &Connection, sql: &str) -> Result<HashMap<i64, Vec<String>>> {
    let mut stmt = conn.prepare(sql)?;
    let mut out: HashMap<i64, Vec<String>> = HashMap::new();
    for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
        let (k, v) = row?;
        out.entry(k).or_default().push(v);
    }
    Ok(out)
}

fn load_assets(conn: &Connection, progress: Progress) -> Result<HashMap<String, Asset>> {
    let keywords = match find_join(conn, "ASSETATTRIBUTES", "KEYWORDS")? {
        Some((t, a, k)) => grouped(
            conn,
            &format!(
                "SELECT j.{a}, kw.ZTITLE FROM {t} j JOIN ZKEYWORD kw ON kw.Z_PK = j.{k} ORDER BY kw.ZTITLE"
            ),
        )?,
        None => {
            progress(Event::Warning {
                text: "keyword table not found, keywords unavailable".into(),
            });
            HashMap::new()
        }
    };
    let albums = match find_join(conn, "ALBUMS", "ASSETS")? {
        Some((t, al, a)) => grouped(
            conn,
            &format!(
                "SELECT j.{a}, g.ZTITLE FROM {t} j JOIN ZGENERICALBUM g ON g.Z_PK = j.{al} WHERE g.ZKIND = 2 AND g.ZTITLE IS NOT NULL ORDER BY g.ZTITLE"
            ),
        )?,
        None => {
            progress(Event::Warning {
                text: "album table not found, albums unavailable".into(),
            });
            HashMap::new()
        }
    };
    let mut stmt = conn.prepare(
        "SELECT a.Z_PK, aa.Z_PK, a.ZUUID, COALESCE(aa.ZORIGINALFILENAME, a.ZFILENAME), aa.ZTITLE, d.ZLONGDESCRIPTION,
                a.ZDATECREATED, aa.ZTIMEZONEOFFSET, a.ZLATITUDE, a.ZLONGITUDE, a.ZFAVORITE, a.ZKIND, aa.ZREVERSELOCATIONDATA,
                e.ZCAMERAMAKE, e.ZCAMERAMODEL, e.ZLENSMODEL
         FROM ZASSET a
         JOIN ZADDITIONALASSETATTRIBUTES aa ON aa.ZASSET = a.Z_PK
         LEFT JOIN ZASSETDESCRIPTION d ON d.ZASSETATTRIBUTES = aa.Z_PK
         LEFT JOIN ZEXTENDEDATTRIBUTES e ON e.ZASSET = a.Z_PK
         WHERE a.ZTRASHEDSTATE = 0",
    )?;
    let rows = stmt.query_map([], |r| {
        let pk: i64 = r.get(0)?;
        let attr_pk: i64 = r.get(1)?;
        let coord = |i: usize| -> rusqlite::Result<Option<f64>> {
            Ok(r.get::<_, Option<f64>>(i)?.filter(|c| *c > -180.0))
        };
        Ok((
            pk,
            attr_pk,
            Asset {
                uuid: r.get(2)?,
                filename: r.get(3)?,
                title: r.get(4)?,
                description: r.get(5)?,
                taken: cocoa_time(
                    r.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
                    r.get::<_, Option<i32>>(7)?.unwrap_or(0),
                ),
                latitude: coord(8)?,
                longitude: coord(9)?,
                favorite: r.get::<_, Option<i64>>(10)? == Some(1),
                video: r.get::<_, Option<i64>>(11)? == Some(1),
                camera: Camera {
                    make: r.get(13)?,
                    model: r.get(14)?,
                    lens: r.get(15)?,
                },
                place: r
                    .get::<_, Option<Vec<u8>>>(12)?
                    .map(|b| super::place::place(&b))
                    .unwrap_or_default(),
                ..Default::default()
            },
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (pk, attr_pk, mut asset) = row?;
        asset.fill_place();
        asset.keywords = keywords.get(&attr_pk).cloned().unwrap_or_default();
        asset.albums = albums.get(&pk).cloned().unwrap_or_default();
        out.insert(asset.uuid.clone(), asset);
    }
    Ok(out)
}

#[derive(Debug, Default, PartialEq)]
struct Indexed {
    labels: Vec<String>,
    people: Vec<String>,
}

fn load_index(conn: &Connection) -> Result<HashMap<String, Indexed>> {
    let mut stmt = conn.prepare("SELECT lexeme_id, category, content FROM lexicon WHERE category IN (3000, 4000) AND type = 1 AND content != ''")?;
    let lexicon: HashMap<u32, (i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
        .collect::<Result<_, _>>()?;
    let mut stmt = conn.prepare("SELECT identifier, lexeme_ids, lexeme_scores FROM items WHERE lexeme_ids IS NOT NULL AND lexeme_scores IS NOT NULL")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Vec<u8>>(1)?,
            r.get::<_, Vec<u8>>(2)?,
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (uuid, ids, scores) = row?;
        let mut hits: Vec<(u16, usize, &(i64, String))> = ids
            .as_chunks::<4>()
            .0
            .iter()
            .zip(scores.as_chunks::<2>().0)
            .enumerate()
            .filter_map(|(i, (id, score))| {
                Some((
                    u16::from_le_bytes(*score),
                    i,
                    lexicon.get(&u32::from_le_bytes(*id))?,
                ))
            })
            .collect();
        if hits.is_empty() {
            continue;
        }
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        let mut indexed = Indexed::default();
        for (_, _, (category, content)) in hits {
            match category {
                3000 => &mut indexed.people,
                _ => &mut indexed.labels,
            }
            .push(content.clone());
        }
        out.insert(uuid, indexed);
    }
    Ok(out)
}

fn copy_db(src: &Path, dir: &Path) -> Result<Option<Connection>> {
    if !src.exists() {
        return Ok(None);
    }
    let name = src.file_name().unwrap().to_string_lossy();
    for suffix in ["", "-wal", "-shm"] {
        let from = src.with_file_name(format!("{name}{suffix}"));
        if from.exists() {
            std::fs::copy(&from, dir.join(from.file_name().unwrap()))
                .with_context(|| format!("copy {}", from.display()))?;
        }
    }
    Ok(Some(Connection::open_with_flags(
        dir.join(&*name),
        OpenFlags::SQLITE_OPEN_READ_WRITE,
    )?))
}

pub(crate) fn load(library: &Path, progress: Progress) -> Result<HashMap<String, Asset>> {
    let db = library.join("database");
    let tmp = tempfile::tempdir()?;
    let photos = copy_db(&db.join("Photos.sqlite"), tmp.path())?
        .context("Photos.sqlite not found (grant Full Disk Access to Photosink)")?;
    let mut assets = load_assets(&photos, progress)?;
    match copy_db(&db.join("search/leo.sqlite"), tmp.path())? {
        Some(search) => {
            for (uuid, indexed) in load_index(&search)? {
                if let Some(asset) = assets.get_mut(&uuid) {
                    asset.labels = indexed.labels;
                    asset.people = indexed.people;
                }
            }
        }
        None => progress(Event::Warning {
            text: "search index not found, labels and people unavailable".into(),
        }),
    }
    Ok(assets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::photo::Place;

    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE ZASSET (Z_PK INTEGER PRIMARY KEY, ZUUID TEXT, ZFILENAME TEXT, ZDATECREATED REAL, ZLATITUDE REAL, ZLONGITUDE REAL, ZFAVORITE INTEGER, ZKIND INTEGER, ZTRASHEDSTATE INTEGER);
             CREATE TABLE ZADDITIONALASSETATTRIBUTES (Z_PK INTEGER PRIMARY KEY, ZASSET INTEGER, ZORIGINALFILENAME TEXT, ZTITLE TEXT, ZTIMEZONEOFFSET INTEGER, ZREVERSELOCATIONDATA BLOB);
             CREATE TABLE ZASSETDESCRIPTION (Z_PK INTEGER PRIMARY KEY, ZASSETATTRIBUTES INTEGER, ZLONGDESCRIPTION TEXT);
             CREATE TABLE ZKEYWORD (Z_PK INTEGER PRIMARY KEY, ZTITLE TEXT);
             CREATE TABLE Z_1KEYWORDS (Z_1ASSETATTRIBUTES INTEGER, Z_40KEYWORDS INTEGER);
             CREATE TABLE ZGENERICALBUM (Z_PK INTEGER PRIMARY KEY, ZKIND INTEGER, ZTITLE TEXT);
             CREATE TABLE Z_28ASSETS (Z_28ALBUMS INTEGER, Z_3ASSETS INTEGER, Z_FOK_3ASSETS INTEGER);
             CREATE TABLE ZEXTENDEDATTRIBUTES (Z_PK INTEGER PRIMARY KEY, ZASSET INTEGER, ZCAMERAMAKE TEXT, ZCAMERAMODEL TEXT, ZLENSMODEL TEXT);
             INSERT INTO ZEXTENDEDATTRIBUTES VALUES (1, 1, 'Canon', 'EOS R5', 'RF24-70mm F2.8 L IS USM');
             INSERT INTO ZASSET VALUES (1, 'U1', 'x.heic', 0, -180.0, -180.0, 1, 0, 0);
             INSERT INTO ZASSET VALUES (2, 'U2', 'y.mov', 86400, 47.6785, -122.1305, 0, 1, 0);
             INSERT INTO ZASSET VALUES (3, 'U3', 'z.jpg', 0, 0, 0, 0, 0, 1);
             INSERT INTO ZADDITIONALASSETATTRIBUTES VALUES (10, 1, 'IMG_1.HEIC', 'Title', 3600, NULL);
             INSERT INTO ZADDITIONALASSETATTRIBUTES VALUES (20, 2, NULL, NULL, 0, NULL);
             INSERT INTO ZADDITIONALASSETATTRIBUTES VALUES (30, 3, 'z.jpg', NULL, 0, NULL);
             INSERT INTO ZASSETDESCRIPTION VALUES (1, 10, 'desc');
             INSERT INTO ZKEYWORD VALUES (1, 'beach'), (2, 'sunset');
             INSERT INTO Z_1KEYWORDS VALUES (10, 2), (10, 1);
             INSERT INTO ZGENERICALBUM VALUES (1, 2, 'Trip'), (2, 4000, 'Smart');
             INSERT INTO Z_28ASSETS VALUES (1, 1, 0), (2, 1, 0);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn loads_index_by_score_then_specificity() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE lexicon (lexeme_id INTEGER, type INTEGER, category INTEGER, content TEXT);
             CREATE TABLE items (identifier TEXT, lexeme_ids BLOB, lexeme_scores BLOB);
             INSERT INTO lexicon VALUES (1, 1, 4000, 'Animal'), (2, 1, 4000, 'Cat'), (3, 1, 4000, 'People'), (3, 2, 4000, 'Persons'), (4, 1, 2090, 'Redmond'), (5, 1, 3000, 'Kairi'), (6, 1, 3000, '');
             INSERT INTO items VALUES ('U1', X'010000000200000003000000040000000500000006000000', X'100010002000000030003000'), ('U2', X'04000000', X'0000'), ('U3', NULL, NULL);",
        )
        .unwrap();
        let index = load_index(&conn).unwrap();
        assert_eq!(
            index["U1"],
            Indexed {
                labels: vec!["People".into(), "Cat".into(), "Animal".into()],
                people: vec!["Kairi".into()]
            }
        );
        assert!(!index.contains_key("U2"));
    }

    #[test]
    fn loads_assets_with_relations() {
        let assets = load_assets(&fixture(), &|_| {}).unwrap();
        assert_eq!(assets.len(), 2);
        let a = &assets["U1"];
        assert_eq!(a.filename, "IMG_1.HEIC");
        assert_eq!(a.stem(), "IMG_1");
        assert_eq!(a.title.as_deref(), Some("Title"));
        assert_eq!(a.description.as_deref(), Some("desc"));
        assert_eq!(a.keywords, ["beach", "sunset"]);
        assert_eq!(a.albums, ["Trip"]);
        assert_eq!(a.camera.lens.as_deref(), Some("RF24-70mm F2.8 L IS USM"));
        assert_eq!(a.field("camera.model", 0).as_deref(), Some("EOS R5"));
        assert!(a.favorite && !a.video);
        assert_eq!(a.latitude, None);
        assert_eq!(a.field("take.date", 0).as_deref(), Some("2001-01-01"));
        assert_eq!(a.taken.unwrap().offset().local_minus_utc(), 3600);
        let b = &assets["U2"];
        assert_eq!(b.filename, "y.mov");
        assert!(b.video && b.keywords.is_empty());
        assert_eq!(b.camera, Camera::default());
        assert_eq!(b.latitude, Some(47.6785));
        assert_eq!(b.place.country.as_deref(), Some("US"));
        assert_eq!(a.place, Place::default());
        assert_eq!(b.field("take.day", 0).as_deref(), Some("02"));
        assert_eq!(b.field("type", 0).as_deref(), Some("video"));
        assert_eq!(b.field("keyword", 0), None);
    }
}
