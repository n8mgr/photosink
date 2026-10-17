use plist::{Dictionary, Value};
use std::io::Cursor;

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Place {
    pub name: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub country: Option<String>,
}

fn varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut out = 0u64;
    for shift in (0..64).step_by(7) {
        let b = *buf.get(*pos)?;
        *pos += 1;
        out |= u64::from(b & 0x7f) << shift;
        if b < 0x80 {
            return Some(out);
        }
    }
    None
}

fn fields(buf: &[u8]) -> Vec<(u64, &[u8])> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        let Some(key) = varint(buf, &mut pos) else {
            break;
        };
        let skip = match key & 7 {
            0 => {
                varint(buf, &mut pos);
                0
            }
            1 => 8,
            5 => 4,
            2 => {
                let Some(len) = varint(buf, &mut pos).map(|l| l as usize) else {
                    break;
                };
                let Some(bytes) = buf.get(pos..pos + len) else {
                    break;
                };
                out.push((key >> 3, bytes));
                len
            }
            _ => break,
        };
        pos += skip;
    }
    out
}

fn get<'a>(buf: &'a [u8], path: &[u64]) -> Option<&'a [u8]> {
    path.iter().try_fold(buf, |msg, f| {
        fields(msg)
            .into_iter()
            .find(|(k, _)| k == f)
            .map(|(_, v)| v)
    })
}

fn string(buf: &[u8], path: &[u64]) -> Option<String> {
    std::str::from_utf8(get(buf, path)?)
        .ok()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn unarchive(dict: &Dictionary) -> Option<Value> {
    let objects = dict.get("$objects")?.as_array()?;
    let top = dict.get("$top")?.as_dictionary()?.get("root")?.as_uid()?;
    resolve(objects, top.get(), 0)
}

fn resolve(objects: &[Value], idx: u64, depth: u32) -> Option<Value> {
    if idx == 0 || depth > 64 {
        return None;
    }
    let follow = |v: &Value| match v {
        Value::Uid(u) => resolve(objects, u.get(), depth + 1),
        other => Some(other.clone()),
    };
    match objects.get(idx as usize)? {
        Value::Dictionary(d) => {
            let items = |k: &str| {
                d.get(k)
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(follow).collect::<Vec<_>>())
            };
            match (items("NS.keys"), items("NS.objects")) {
                (Some(keys), Some(vals)) => Some(Value::Dictionary(
                    keys.into_iter()
                        .zip(vals)
                        .filter_map(|(k, v)| Some((k?.as_string()?.to_owned(), v?)))
                        .collect(),
                )),
                (None, Some(vals)) => Some(Value::Array(vals.into_iter().flatten().collect())),
                _ => Some(Value::Dictionary(
                    d.iter()
                        .filter(|(k, _)| *k != "$class")
                        .filter_map(|(k, v)| Some((k.clone(), follow(v)?)))
                        .collect(),
                )),
            }
        }
        v => follow(v),
    }
}

fn value_at(v: &Value, path: &[&str]) -> Option<String> {
    let mut cur = v;
    for p in path {
        cur = match p.parse::<usize>() {
            Ok(i) => cur.as_array()?.get(i)?,
            Err(_) => cur.as_dictionary()?.get(p)?,
        };
    }
    cur.as_string().filter(|s| !s.is_empty()).map(str::to_owned)
}

fn archived_place(dict: &Dictionary) -> Place {
    let Some(root) = unarchive(dict) else {
        return Place::default();
    };
    Place {
        name: value_at(&root, &["mapItem", "sortedPlaceInfos", "0", "name"]),
        city: value_at(&root, &["postalAddress", "_city"]),
        state: value_at(&root, &["postalAddress", "_state"]),
        country: value_at(&root, &["postalAddress", "_country"]),
    }
}

fn protobuf_place(pb: &[u8]) -> Place {
    Place {
        name: string(pb, &[3, 4]),
        city: string(pb, &[1, 6, 15, 5]),
        state: string(pb, &[1, 6, 15, 3]),
        country: string(pb, &[1, 6, 15, 1]),
    }
}

pub fn nearest(lat: f64, lon: f64) -> Place {
    static GEOCODER: std::sync::OnceLock<reverse_geocoder::ReverseGeocoder> =
        std::sync::OnceLock::new();
    let r = GEOCODER
        .get_or_init(reverse_geocoder::ReverseGeocoder::new)
        .search((lat, lon))
        .record;
    let some = |s: &str| Some(s.to_owned()).filter(|s| !s.is_empty());
    Place {
        name: some(&r.name),
        city: some(&r.name),
        state: some(&r.admin1),
        country: some(&r.cc),
    }
}

pub fn place(blob: &[u8]) -> Place {
    let Some(Value::Dictionary(dict)) = Value::from_reader(Cursor::new(blob)).ok() else {
        return Place::default();
    };
    match dict.get("geoPlaceResult").and_then(Value::as_data) {
        Some(pb) => protobuf_place(pb),
        None => archived_place(&dict),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(field: u64, body: &[u8]) -> Vec<u8> {
        let mut out = vec![(field << 3) as u8 | 2, body.len() as u8];
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn decodes_place() {
        let postal = [
            msg(1, b"United States"),
            msg(3, b"California"),
            msg(5, b"San Francisco"),
        ]
        .concat();
        let result = [
            vec![0x18, 0x31],
            msg(4, b"Great Hwy"),
            msg(6, &msg(15, &postal)),
        ]
        .concat();
        let info = [
            vec![0x18, 0x2b],
            msg(4, b"Outer Sunset"),
            vec![0x89, 0x01, 0, 0, 0, 0, 0, 0, 0, 0],
        ]
        .concat();
        let pb = [
            msg(1, &result),
            msg(3, &info),
            msg(3, &msg(4, b"California")),
        ]
        .concat();
        let mut dict = plist::Dictionary::new();
        dict.insert("geoPlaceResult".into(), Value::Data(pb));
        let mut blob = Vec::new();
        plist::to_writer_binary(&mut blob, &Value::Dictionary(dict)).unwrap();
        assert_eq!(
            place(&blob),
            Place {
                name: Some("Outer Sunset".into()),
                city: Some("San Francisco".into()),
                state: Some("California".into()),
                country: Some("United States".into())
            }
        );
    }

    #[test]
    fn decodes_archived_place() {
        use plist::Uid;
        let uid = |n: u64| Value::Uid(Uid::new(n));
        let obj = |pairs: Vec<(&str, Value)>| {
            Value::Dictionary(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
        };
        let objects = vec![
            "$null".into(),
            obj(vec![
                ("$class", uid(9)),
                ("mapItem", uid(2)),
                ("postalAddress", uid(5)),
            ]),
            obj(vec![("sortedPlaceInfos", uid(3))]),
            obj(vec![("NS.objects", Value::Array(vec![uid(4), uid(0)]))]),
            obj(vec![
                ("name", uid(6)),
                ("placeType", Value::Integer(9.into())),
            ]),
            obj(vec![
                ("_city", uid(7)),
                ("_state", uid(0)),
                ("_country", uid(8)),
            ]),
            "Lake Sammamish".into(),
            "Issaquah".into(),
            "United States".into(),
            obj(vec![("$classname", "PLRevGeoLocationInfo".into())]),
        ];
        let mut root = Dictionary::new();
        root.insert("$archiver".into(), "NSKeyedArchiver".into());
        root.insert("$objects".into(), Value::Array(objects));
        root.insert("$top".into(), obj(vec![("root", uid(1))]));
        let mut blob = Vec::new();
        plist::to_writer_binary(&mut blob, &Value::Dictionary(root)).unwrap();
        assert_eq!(
            place(&blob),
            Place {
                name: Some("Lake Sammamish".into()),
                city: Some("Issaquah".into()),
                state: None,
                country: Some("United States".into())
            }
        );
    }

    #[test]
    fn nearest_city() {
        let p = nearest(47.6785, -122.1305);
        assert_eq!(p.country.as_deref(), Some("US"));
        assert_eq!(p.state.as_deref(), Some("Washington"));
        assert!(p.city.is_some());
        assert_eq!(p.name, p.city);
    }

    #[test]
    fn tolerates_garbage() {
        assert_eq!(place(b"junk"), Place::default());
        assert_eq!(string(&[0x0a, 0xff], &[1]), None);
    }
}
