use super::{padded, replace_file, same};
use anyhow::{Result, bail};
use std::path::Path;

const KEYWORD: &[u8] = b"XML:com.adobe.xmp";

struct Layout {
    xmp: Option<(std::ops::Range<usize>, usize)>,
    insert_at: usize,
}

fn layout(data: &[u8]) -> Result<Layout> {
    let mut pos = 8;
    let mut insert_at = None;
    let mut xmp = None;
    while pos + 12 <= data.len() {
        let len = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        let kind = &data[pos + 4..pos + 8];
        let end = pos + 12 + len;
        if end > data.len() {
            bail!("truncated chunk at {pos}");
        }
        let body = &data[pos + 8..end - 4];
        if kind == b"IDAT" && insert_at.is_none() {
            insert_at = Some(pos);
        }
        if kind == b"iTXt" && body.starts_with(KEYWORD) && body.get(KEYWORD.len()) == Some(&0) {
            // keyword NUL, compression flag, compression method, language NUL, translated NUL
            let mut p = KEYWORD.len() + 3;
            let compressed = body.get(KEYWORD.len() + 1) == Some(&1);
            for _ in 0..2 {
                p += body[p.min(body.len())..]
                    .iter()
                    .position(|b| *b == 0)
                    .unwrap_or(0)
                    + 1;
            }
            if xmp.is_none() && !compressed && p <= body.len() {
                xmp = Some((pos..end, pos + 8 + p));
            }
        }
        if kind == b"IEND" {
            break;
        }
        pos = end;
    }
    Ok(Layout {
        xmp,
        insert_at: insert_at.unwrap_or(pos),
    })
}

fn chunk(payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(KEYWORD.len() + 5 + payload.len());
    body.extend_from_slice(KEYWORD);
    body.extend_from_slice(&[0, 0, 0, 0, 0]);
    body.extend_from_slice(payload);
    let mut out = Vec::with_capacity(body.len() + 12);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(b"iTXt");
    let crc = {
        let mut h = crc32fast::Hasher::new();
        h.update(b"iTXt");
        h.update(&body);
        h.finalize()
    };
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

pub fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    let data = std::fs::read(path)?;
    Ok(layout(&data)?
        .xmp
        .map(|(range, text_at)| data[text_at..range.end - 4].to_vec()))
}

pub fn embed(path: &Path, xmp: &str) -> Result<bool> {
    let mut data = std::fs::read(path)?;
    let layout = layout(&data)?;
    match layout.xmp {
        Some((range, text_at)) => {
            if same(&data[text_at..range.end - 4], xmp) {
                return Ok(false);
            }
            data.splice(range, chunk(&padded(xmp)));
        }
        None => {
            data.splice(layout.insert_at..layout.insert_at, chunk(&padded(xmp)));
        }
    }
    replace_file(path, &data)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_chunk(kind: &[u8], body: &[u8]) -> Vec<u8> {
        let mut out = (body.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        let mut h = crc32fast::Hasher::new();
        h.update(kind);
        h.update(body);
        out.extend_from_slice(&h.finalize().to_be_bytes());
        out
    }

    fn png() -> Vec<u8> {
        let mut d = b"\x89PNG\r\n\x1a\n".to_vec();
        d.extend(raw_chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]));
        d.extend(raw_chunk(
            b"IDAT",
            &[8, 29, 1, 2, 0, 253, 255, 0, 0, 0, 2, 0, 1],
        ));
        d.extend(raw_chunk(b"IEND", &[]));
        d
    }

    #[test]
    fn inserts_before_idat_with_valid_crc_then_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.png");
        std::fs::write(&p, png()).unwrap();
        let x1 = "<x>1</x>\n<?xpacket end=\"w\"?>\n";
        assert!(embed(&p, x1).unwrap());
        let d = std::fs::read(&p).unwrap();
        let l = layout(&d).unwrap();
        let (range, text_at) = l.xmp.clone().unwrap();
        assert_eq!(range.start, 8 + 25, "right after IHDR");
        assert_eq!(l.insert_at, range.end, "IDAT follows");
        assert!(d[text_at..].starts_with(b"<x>1</x>"));
        let body = &d[range.start + 8..range.end - 4];
        let mut h = crc32fast::Hasher::new();
        h.update(b"iTXt");
        h.update(body);
        assert_eq!(&d[range.end - 4..range.end], h.finalize().to_be_bytes());
        assert!(same(&read(&p).unwrap().unwrap(), x1));
        assert!(!embed(&p, x1).unwrap());
        assert!(embed(&p, "<x>2</x>\n<?xpacket end=\"w\"?>\n").unwrap());
        let d2 = std::fs::read(&p).unwrap();
        assert_eq!(d2.len(), d.len());
        assert_eq!(super::super::find(&d2, b"<x>1</x>"), None);
        assert!(d2.ends_with(&raw_chunk(b"IEND", &[])));
    }
}
