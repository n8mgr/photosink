use crate::photo::Camera;
use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone};
use exif::{Context, Exif, Field, Reader, Value};
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

fn head(file: &mut BufReader<File>) -> Vec<u8> {
    let mut head = vec![0u8; 4 << 20];
    let n = file
        .rewind()
        .and_then(|_| file.read(&mut head))
        .unwrap_or(0);
    head.truncate(n);
    head
}

fn cr3_tiff_boxes(head: &[u8]) -> Vec<Vec<u8>> {
    [b"CMT1", b"CMT2", b"CMT4"]
        .iter()
        .filter_map(|name| {
            let i = head.windows(4).position(|w| w == *name)?;
            let size = u32::from_be_bytes(head[i.checked_sub(4)?..i].try_into().ok()?) as usize;
            head.get(i + 4..i + size - 4).map(<[u8]>::to_vec)
        })
        .collect()
}

fn parse(data: Vec<u8>) -> Vec<Exif> {
    Reader::new().read_raw(data).ok().into_iter().collect()
}

fn read(path: &Path) -> Vec<Exif> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let mut file = BufReader::new(file);
    if let Ok(exif) = Reader::new().read_from_container(&mut file) {
        return vec![exif];
    }
    let mut head = head(&mut file);
    match head.get(..4) {
        Some(b"IIRO" | b"IIRS" | b"IIU\0") => {
            head[2..4].copy_from_slice(&[0x2a, 0]);
            parse(head)
        }
        Some(b"MMOR") => {
            head[2..4].copy_from_slice(&[0, 0x2a]);
            parse(head)
        }
        _ if head.starts_with(b"FUJIFILMCCD-RAW") => {
            let offset = head
                .get(84..88)
                .map(|b| u64::from(u32::from_be_bytes(b.try_into().unwrap())));
            offset
                .and_then(|o| file.seek(std::io::SeekFrom::Start(o)).ok())
                .and_then(|_| Reader::new().read_from_container(&mut file).ok())
                .into_iter()
                .collect()
        }
        _ => cr3_tiff_boxes(&head).into_iter().flat_map(parse).collect(),
    }
}

fn field<'a>(exifs: &'a [Exif], number: u16, contexts: &[Context]) -> Option<&'a Field> {
    exifs
        .iter()
        .flat_map(Exif::fields)
        .find(|f| f.tag.number() == number && contexts.contains(&f.tag.context()))
}

fn text(exifs: &[Exif], number: u16, contexts: &[Context]) -> Option<String> {
    match &field(exifs, number, contexts)?.value {
        Value::Ascii(v) => Some(
            String::from_utf8_lossy(v.first()?)
                .trim_end_matches(['\0', ' '])
                .to_owned(),
        )
        .filter(|s| !s.is_empty()),
        _ => None,
    }
}

fn coordinate(exifs: &[Exif], value_tag: u16, ref_tag: u16) -> Option<f64> {
    let contexts = [Context::Gps, Context::Tiff];
    let degrees = match &field(exifs, value_tag, &contexts)?.value {
        Value::Rational(v) if v.len() == 3 => {
            v[0].to_f64() + v[1].to_f64() / 60.0 + v[2].to_f64() / 3600.0
        }
        _ => return None,
    };
    let south_or_west = matches!(text(exifs, ref_tag, &contexts).as_deref(), Some("S" | "W"));
    Some(if south_or_west { -degrees } else { degrees })
}

fn date(exifs: &[Exif], number: u16) -> Option<NaiveDateTime> {
    let s = text(exifs, number, &[Context::Tiff, Context::Exif])?;
    NaiveDateTime::parse_from_str(s.trim(), "%Y:%m:%d %H:%M:%S").ok()
}

fn local(naive: NaiveDateTime) -> Option<DateTime<FixedOffset>> {
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|t| t.fixed_offset())
}

pub struct Metadata {
    pub gps: Option<(f64, f64)>,
    pub camera: Camera,
}

#[derive(Debug, Default, PartialEq)]
pub struct Dates {
    pub original: Option<NaiveDateTime>,
    pub create: Option<NaiveDateTime>,
    pub modify: Option<NaiveDateTime>,
}

/// EXIF DateTimeOriginal, DateTimeDigitized and ModifyDate; for a video
/// container without EXIF, the `mvhd` creation time as `create`.
pub fn dates(path: &Path) -> Dates {
    let exifs = read(path);
    let mut d = Dates {
        original: date(&exifs, 0x9003),
        create: date(&exifs, 0x9004),
        modify: date(&exifs, 0x0132),
    };
    if d == Dates::default() {
        d.create =
            crate::photo::embed::creation_time(path).map(|t| t.with_timezone(&Local).naive_local());
    }
    d
}

/// The best taken date the file itself offers, in the local zone.
pub fn taken(path: &Path) -> Option<DateTime<FixedOffset>> {
    let d = dates(path);
    d.original.or(d.create).or(d.modify).and_then(local)
}

pub fn read_metadata(path: &Path) -> Metadata {
    let exifs = read(path);
    let tiff_or_exif = [Context::Tiff, Context::Exif];
    Metadata {
        gps: coordinate(&exifs, 2, 1).zip(coordinate(&exifs, 4, 3)),
        camera: Camera {
            make: text(&exifs, 0x010f, &tiff_or_exif),
            model: text(&exifs, 0x0110, &tiff_or_exif),
            lens: text(&exifs, 0xa434, &tiff_or_exif),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(tag: u16, kind: u16, count: u32, value: u32) -> Vec<u8> {
        [
            tag.to_le_bytes().as_slice(),
            &kind.to_le_bytes(),
            &count.to_le_bytes(),
            &value.to_le_bytes(),
        ]
        .concat()
    }

    fn rationals(v: [(u32, u32); 3]) -> Vec<u8> {
        v.iter()
            .flat_map(|(n, d)| [n.to_le_bytes(), d.to_le_bytes()].concat())
            .collect()
    }

    fn gps_ifd(at: u32) -> Vec<u8> {
        let data = at + 2 + 4 * 12 + 4;
        [
            4u16.to_le_bytes().to_vec(),
            entry(1, 2, 2, u32::from_le_bytes(*b"N\0\0\0")),
            entry(2, 5, 3, data),
            entry(3, 2, 2, u32::from_le_bytes(*b"W\0\0\0")),
            entry(4, 5, 3, data + 24),
            0u32.to_le_bytes().to_vec(),
            rationals([(47, 1), (40, 1), (4260, 100)]),
            rationals([(122, 1), (7, 1), (50, 1)]),
        ]
        .concat()
    }

    fn ifd0(gps_offset: Option<u32>) -> Vec<u8> {
        let mut entries = vec![entry(0x010f, 2, 6, 0), entry(0x0110, 2, 7, 0)];
        entries.extend(gps_offset.map(|o| entry(0x8825, 4, 1, o)));
        let text_at = 8 + 2 + 12 * entries.len() as u32 + 4;
        entries[0][8..].copy_from_slice(&text_at.to_le_bytes());
        entries[1][8..].copy_from_slice(&(text_at + 6).to_le_bytes());
        [
            (entries.len() as u16).to_le_bytes().to_vec(),
            entries.concat(),
            0u32.to_le_bytes().to_vec(),
            b"Canon\0EOS R5\0".to_vec(),
        ]
        .concat()
    }

    fn tiff() -> Vec<u8> {
        let header = [b"II*\0".to_vec(), 8u32.to_le_bytes().to_vec()].concat();
        let gps_at = (header.len() + ifd0(Some(0)).len()) as u32;
        [header, ifd0(Some(gps_at)), gps_ifd(gps_at)].concat()
    }

    fn check(m: &Metadata) {
        let (lat, lon) = m.gps.unwrap();
        assert!(
            (lat - 47.6785).abs() < 1e-4 && (lon + 122.1306).abs() < 1e-4,
            "{lat} {lon}"
        );
    }

    #[test]
    fn reads_tiff() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.tif");
        std::fs::write(&p, tiff()).unwrap();
        let m = read_metadata(&p);
        check(&m);
        assert_eq!(
            m.camera,
            Camera {
                make: Some("Canon".into()),
                model: Some("EOS R5".into()),
                lens: None
            }
        );
    }

    #[test]
    fn reads_cr3_boxes() {
        let boxed = |name: &[u8], body: Vec<u8>| {
            [
                ((body.len() + 8) as u32).to_be_bytes().to_vec(),
                name.to_vec(),
                body,
            ]
            .concat()
        };
        let cmt1 = boxed(
            b"CMT1",
            [b"II*\0".to_vec(), 8u32.to_le_bytes().to_vec(), ifd0(None)].concat(),
        );
        let cmt4 = boxed(
            b"CMT4",
            [b"II*\0".to_vec(), 8u32.to_le_bytes().to_vec(), gps_ifd(8)].concat(),
        );
        let file = [
            b"\0\0\0\x18ftypcrx \0\0\0\x01crx isom".to_vec(),
            vec![0u8; 100],
            cmt1,
            cmt4,
        ]
        .concat();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.cr3");
        std::fs::write(&p, file).unwrap();
        let m = read_metadata(&p);
        check(&m);
        assert_eq!(m.camera.model.as_deref(), Some("EOS R5"));
    }

    fn write(name: &str, data: Vec<u8>) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(name);
        std::fs::write(&p, data).unwrap();
        (dir, p)
    }

    #[test]
    fn reads_orf_and_rw2_magic() {
        for (name, magic) in [("a.orf", b"IIRO"), ("a.rw2", b"IIU\0")] {
            let mut data = tiff();
            data[..4].copy_from_slice(magic);
            let (_dir, p) = write(name, data);
            let m = read_metadata(&p);
            check(&m);
            assert_eq!(m.camera.make.as_deref(), Some("Canon"), "{name}");
        }
    }

    #[test]
    fn reads_raf_embedded_jpeg() {
        let tiff = tiff();
        let app1 = [b"Exif\0\0".to_vec(), tiff].concat();
        let jpeg = [
            b"\xff\xd8\xff\xe1".to_vec(),
            ((app1.len() + 2) as u16).to_be_bytes().to_vec(),
            app1,
            b"\xff\xd9".to_vec(),
        ]
        .concat();
        let mut header = b"FUJIFILMCCD-RAW 0201FF393103".to_vec();
        header.resize(84, 0);
        header.extend((160u32).to_be_bytes());
        header.extend((jpeg.len() as u32).to_be_bytes());
        header.resize(160, 0);
        let (_dir, p) = write("a.raf", [header, jpeg].concat());
        let m = read_metadata(&p);
        check(&m);
        assert_eq!(m.camera.model.as_deref(), Some("EOS R5"));
    }

    #[test]
    fn empty_without_exif() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.bin");
        std::fs::write(&p, b"nothing here").unwrap();
        let m = read_metadata(&p);
        assert!(m.gps.is_none() && m.camera == Camera::default());
        assert!(read_metadata(&dir.path().join("missing")).gps.is_none());
    }
}
