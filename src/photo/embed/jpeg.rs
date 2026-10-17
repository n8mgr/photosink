use super::{padded, replace_file, same};
use anyhow::{Result, bail};
use std::path::Path;

const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const EXIF_HEADER: &[u8] = b"Exif\0\0";

struct Layout {
    xmp: Option<std::ops::Range<usize>>,
    insert_at: usize,
}

fn layout(data: &[u8]) -> Result<Layout> {
    let mut pos = 2;
    let mut insert_at = 2;
    let mut xmp = None;
    let mut leading = true;
    while pos + 4 <= data.len() {
        if data[pos] != 0xff {
            bail!("bad marker at {pos}");
        }
        let marker = data[pos + 1];
        match marker {
            0xff => {
                pos += 1;
                continue;
            }
            0xd8 | 0x01 | 0xd0..=0xd7 => {
                pos += 2;
                continue;
            }
            0xd9 | 0xda => break,
            _ => {}
        }
        let len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        let end = pos + 2 + len;
        if len < 2 || end > data.len() {
            bail!("truncated segment at {pos}");
        }
        let payload = &data[pos + 4..end];
        if marker == 0xe1 && payload.starts_with(XMP_HEADER) {
            if xmp.is_none() {
                xmp = Some(pos..end);
            }
            leading = false;
        } else if leading
            && (marker == 0xe0 || (marker == 0xe1 && payload.starts_with(EXIF_HEADER)))
        {
            insert_at = end;
        } else {
            leading = false;
        }
        pos = end;
    }
    Ok(Layout { xmp, insert_at })
}

fn segment(payload: &[u8]) -> Result<Vec<u8>> {
    let len = 2 + XMP_HEADER.len() + payload.len();
    if len > 0xffff {
        bail!(
            "XMP packet of {} bytes does not fit a JPEG segment",
            payload.len()
        );
    }
    let mut seg = Vec::with_capacity(len + 2);
    seg.extend_from_slice(&[0xff, 0xe1]);
    seg.extend_from_slice(&(len as u16).to_be_bytes());
    seg.extend_from_slice(XMP_HEADER);
    seg.extend_from_slice(payload);
    Ok(seg)
}

pub fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    let data = std::fs::read(path)?;
    Ok(layout(&data)?
        .xmp
        .map(|r| data[r.start + 4 + XMP_HEADER.len()..r.end].to_vec()))
}

pub fn embed(path: &Path, xmp: &str) -> Result<bool> {
    let mut data = std::fs::read(path)?;
    let layout = layout(&data)?;
    let seg = match layout.xmp {
        Some(range) => {
            let existing = &data[range.start + 4 + XMP_HEADER.len()..range.end];
            if same(existing, xmp) {
                return Ok(false);
            }
            let seg = segment(&padded(xmp))?;
            data.splice(range, seg);
            data
        }
        None => {
            let seg = segment(&padded(xmp))?;
            data.splice(layout.insert_at..layout.insert_at, seg);
            data
        }
    };
    replace_file(path, &seg)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut s = vec![0xff, marker];
        s.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        s.extend_from_slice(payload);
        s
    }

    fn jpeg() -> Vec<u8> {
        let mut d = vec![0xff, 0xd8];
        d.extend(seg(0xe0, b"JFIF\0\x01\x02\0\0\x01\0\x01\0\0"));
        d.extend(seg(0xe1, b"Exif\0\0MM\0*\0\0\0\x08\0\0"));
        d.extend(seg(0xdb, &[0u8; 67]));
        d.extend(seg(0xda, &[0u8; 10]));
        d.extend_from_slice(&[1, 2, 3, 0xff, 0x00, 4, 0xff, 0xd9]);
        d
    }

    #[test]
    fn inserts_after_app_segments_then_replaces_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.jpg");
        std::fs::write(&p, jpeg()).unwrap();
        let x1 = "<x>1</x>\n<?xpacket end=\"w\"?>\n";
        assert!(embed(&p, x1).unwrap());
        let d = std::fs::read(&p).unwrap();
        let l = layout(&d).unwrap();
        let r = l.xmp.clone().unwrap();
        assert_eq!(r.start, 2 + 18 + 20, "after JFIF and Exif");
        assert!(d[r.start + 4..].starts_with(XMP_HEADER));
        assert!(d.ends_with(&[0xff, 0xd9]));
        assert!(same(&read(&p).unwrap().unwrap(), x1));
        assert!(!embed(&p, x1).unwrap(), "unchanged packet is a no-op");
        let x2 = "<x>22</x>\n<?xpacket end=\"w\"?>\n";
        assert!(embed(&p, x2).unwrap());
        let d2 = std::fs::read(&p).unwrap();
        assert_eq!(d2.len(), d.len() + 1);
        assert_eq!(layout(&d2).unwrap().xmp.unwrap().start, r.start);
        assert_eq!(super::super::find(&d2, b"<x>1</x>"), None);
    }

    #[test]
    fn rejects_oversized_packet() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.jpg");
        std::fs::write(&p, jpeg()).unwrap();
        let big = format!("<x>{}</x>\n<?xpacket end=\"w\"?>\n", "a".repeat(70_000));
        assert!(embed(&p, &big).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), jpeg());
    }
}
