use super::{padded, same, splice_file};
use anyhow::{Result, bail};
use std::borrow::Cow;
use std::io::Read;
use std::ops::Range;
use std::path::Path;

const KEYWORD: &[u8] = b"XML:com.adobe.xmp";

struct Xmp {
    chunk: Range<usize>,
    text: Range<usize>,
    compressed: bool,
}

impl Xmp {
    /// The packet, inflated when the chunk is compressed; `None` when it does
    /// not inflate.
    fn packet<'a>(&self, data: &'a [u8]) -> Option<Cow<'a, [u8]>> {
        let text = &data[self.text.clone()];
        if !self.compressed {
            return Some(Cow::Borrowed(text));
        }
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(text)
            .read_to_end(&mut out)
            .ok()?;
        Some(Cow::Owned(out))
    }
}

struct Layout {
    xmp: Option<Xmp>,
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
            if xmp.is_none() && p <= body.len() {
                xmp = Some(Xmp {
                    chunk: pos..end,
                    text: pos + 8 + p..end - 4,
                    compressed,
                });
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
        .and_then(|x| x.packet(&data).map(Cow::into_owned)))
}

pub fn embed(path: &Path, xmp: &str) -> Result<bool> {
    let data = std::fs::read(path)?;
    let layout = layout(&data)?;
    if let Some(p) = layout.xmp.as_ref().and_then(|x| x.packet(&data))
        && same(&p, xmp)
    {
        return Ok(false);
    }
    let range = layout
        .xmp
        .map_or(layout.insert_at..layout.insert_at, |x| x.chunk);
    splice_file(path, data, range, chunk(&padded(xmp)))?;
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
        let x = l.xmp.unwrap();
        let range = x.chunk;
        assert_eq!(range.start, 8 + 25, "right after IHDR");
        assert_eq!(l.insert_at, range.end, "IDAT follows");
        assert!(d[x.text.start..].starts_with(b"<x>1</x>"));
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

    #[test]
    fn replaces_compressed_packet() {
        use flate2::{Compression, write::ZlibEncoder};
        use std::io::Write;
        let old = "<x>old</x>\n<?xpacket end=\"w\"?>\n";
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(old.as_bytes()).unwrap();
        let mut body = KEYWORD.to_vec();
        body.extend_from_slice(&[0, 1, 0, 0, 0]);
        body.extend(z.finish().unwrap());
        let mut d = png();
        let idat = super::super::find(&d, b"IDAT").unwrap() - 4;
        d.splice(idat..idat, raw_chunk(b"iTXt", &body));
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.png");
        std::fs::write(&p, &d).unwrap();
        assert!(same(&read(&p).unwrap().unwrap(), old));
        assert!(!embed(&p, old).unwrap());
        let x = "<x>new</x>\n<?xpacket end=\"w\"?>\n";
        assert!(embed(&p, x).unwrap());
        let d2 = std::fs::read(&p).unwrap();
        assert_eq!(
            d2.windows(4).filter(|w| w == b"iTXt").count(),
            1,
            "one XMP chunk"
        );
        assert!(same(&read(&p).unwrap().unwrap(), x));
    }
}
