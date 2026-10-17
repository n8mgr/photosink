use super::{pad, padded, read_at, same, write_at};
use anyhow::{Result, bail};
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

const XMP_TAG: u16 = 700;
const BYTE: u16 = 1;

#[derive(Clone, Copy)]
struct Order(bool);

impl Order {
    fn u16(self, b: &[u8]) -> u16 {
        let a = [b[0], b[1]];
        if self.0 {
            u16::from_le_bytes(a)
        } else {
            u16::from_be_bytes(a)
        }
    }

    fn u32(self, b: &[u8]) -> u32 {
        let a = [b[0], b[1], b[2], b[3]];
        if self.0 {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        }
    }

    fn put16(self, v: u16) -> [u8; 2] {
        if self.0 {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    }

    fn put32(self, v: u32) -> [u8; 4] {
        if self.0 {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    }
}

fn order(head: &[u8]) -> Option<Order> {
    let order = match head.get(..2)? {
        b"II" => Order(true),
        b"MM" => Order(false),
        _ => return None,
    };
    // 42 is TIFF/DNG/NEF/CR2/ARW; "RO"/"RS" are Olympus ORF, "U\0" is Panasonic RW2.
    let ok = matches!(order.u16(head.get(2..4)?), 42 | 0x4f52 | 0x5352 | 0x55);
    ok.then_some(order)
}

pub fn is_tiff(head: &[u8]) -> bool {
    order(head).is_some()
}

struct Entry {
    tag: u16,
    raw: [u8; 12],
}

struct Ifd0 {
    bo: Order,
    at: u64,
    entries: Vec<Entry>,
    next: [u8; 4],
}

impl Ifd0 {
    fn read(f: &mut File) -> Result<Ifd0> {
        let head = read_at(f, 0, 8)?;
        let Some(bo) = order(&head) else {
            bail!("not a TIFF header");
        };
        let at = u64::from(bo.u32(&head[4..8]));
        let count = bo.u16(&read_at(f, at, 2)?) as usize;
        let table = read_at(f, at + 2, count * 12 + 4)?;
        let entries = (0..count)
            .map(|i| {
                let raw: [u8; 12] = table[i * 12..i * 12 + 12].try_into().unwrap();
                Entry {
                    tag: bo.u16(&raw[..2]),
                    raw,
                }
            })
            .collect();
        Ok(Ifd0 {
            bo,
            at,
            entries,
            next: table[count * 12..].try_into().unwrap(),
        })
    }

    /// Index, file position and length of the XMP tag's data.
    fn xmp(&self) -> Option<(usize, u64, usize)> {
        let i = self.entries.iter().position(|e| e.tag == XMP_TAG)?;
        let raw = self.entries[i].raw;
        let n = self.bo.u32(&raw[4..8]) as usize;
        let pos = if n > 4 {
            u64::from(self.bo.u32(&raw[8..12]))
        } else {
            self.at + 2 + i as u64 * 12 + 8
        };
        Some((i, pos, n))
    }
}

pub fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    let mut f = File::open(path)?;
    match Ifd0::read(&mut f)?.xmp() {
        Some((_, pos, n)) => Ok(Some(read_at(&mut f, pos, n)?)),
        None => Ok(None),
    }
}

pub fn embed(path: &Path, xmp: &str) -> Result<bool> {
    let mut f = OpenOptions::new().read(true).write(true).open(path)?;
    let Ifd0 {
        bo,
        mut entries,
        next,
        ..
    } = Ifd0::read(&mut f)?;
    if let Some((i, pos, n)) = Ifd0::read(&mut f)?.xmp() {
        let existing = read_at(&mut f, pos, n)?;
        if same(&existing, xmp) {
            return Ok(false);
        }
        if n > 4 && n >= xmp.len() {
            write_at(&mut f, pos, &pad(xmp, n))?;
            return Ok(true);
        }
        entries.remove(i);
    }
    let mut end = f.seek(SeekFrom::End(0))?;
    if end % 2 == 1 {
        f.write_all(&[0])?;
        end += 1;
    }
    let payload = padded(xmp);
    let data_at = end;
    f.write_all(&payload)?;
    end += payload.len() as u64;
    if end % 2 == 1 {
        f.write_all(&[0])?;
        end += 1;
    }
    let ifd_at = end;
    if ifd_at + 6 + (entries.len() as u64 + 1) * 12 > u64::from(u32::MAX) {
        bail!("file too large for a 32-bit IFD offset");
    }
    let mut raw = [0u8; 12];
    raw[..2].copy_from_slice(&bo.put16(XMP_TAG));
    raw[2..4].copy_from_slice(&bo.put16(BYTE));
    raw[4..8].copy_from_slice(&bo.put32(payload.len() as u32));
    raw[8..12].copy_from_slice(&bo.put32(data_at as u32));
    entries.push(Entry { tag: XMP_TAG, raw });
    entries.sort_by_key(|e| e.tag);
    let mut out = bo.put16(entries.len() as u16).to_vec();
    for e in &entries {
        out.extend_from_slice(&e.raw);
    }
    out.extend_from_slice(&next);
    f.write_all(&out)?;
    write_at(&mut f, 4, &bo.put32(ifd_at as u32))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiff(le: bool, with_xmp: Option<&[u8]>) -> Vec<u8> {
        let bo = Order(le);
        let mut d = if le { b"II".to_vec() } else { b"MM".to_vec() };
        d.extend_from_slice(&bo.put16(42));
        d.extend_from_slice(&bo.put32(8));
        let mut entries: Vec<[u8; 12]> = Vec::new();
        let mut entry = |tag: u16, typ: u16, count: u32, value: [u8; 4]| {
            let mut raw = [0u8; 12];
            raw[..2].copy_from_slice(&bo.put16(tag));
            raw[2..4].copy_from_slice(&bo.put16(typ));
            raw[4..8].copy_from_slice(&bo.put32(count));
            raw[8..12].copy_from_slice(&value);
            entries.push(raw);
        };
        entry(
            256,
            3,
            1,
            [bo.put16(1), [0, 0]].concat().try_into().unwrap(),
        );
        entry(
            257,
            3,
            1,
            [bo.put16(1), [0, 0]].concat().try_into().unwrap(),
        );
        let n = 3 + u32::from(with_xmp.is_some());
        let data_at = 8 + 2 + n * 12 + 4;
        if let Some(x) = with_xmp {
            entry(700, 1, x.len() as u32, bo.put32(data_at));
        }
        entry(50706, 1, 4, [1, 4, 0, 0]);
        d.extend_from_slice(&bo.put16(entries.len() as u16));
        for e in &entries {
            d.extend_from_slice(e);
        }
        d.extend_from_slice(&bo.put32(0));
        if let Some(x) = with_xmp {
            d.extend_from_slice(x);
        }
        d
    }

    fn read_back(d: &[u8]) -> (Vec<u16>, Vec<u8>) {
        let bo = order(d).unwrap();
        let ifd = bo.u32(&d[4..8]) as usize;
        let count = bo.u16(&d[ifd..]) as usize;
        let mut tags = Vec::new();
        let mut xmp = Vec::new();
        for i in 0..count {
            let e = &d[ifd + 2 + i * 12..];
            tags.push(bo.u16(e));
            if bo.u16(e) == 700 {
                let n = bo.u32(&e[4..]) as usize;
                let at = bo.u32(&e[8..]) as usize;
                xmp = d[at..at + n].to_vec();
            }
        }
        (tags, xmp)
    }

    #[test]
    fn appends_new_ifd0_with_sorted_xmp_entry() {
        for le in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path().join("a.dng");
            std::fs::write(&p, tiff(le, None)).unwrap();
            let x1 = "<x>1</x>\n<?xpacket end=\"w\"?>\n";
            assert!(embed(&p, x1).unwrap());
            let d = std::fs::read(&p).unwrap();
            let orig = tiff(le, None);
            assert_eq!(
                &d[8..orig.len()],
                &orig[8..],
                "only the IFD pointer changed"
            );
            let (tags, xmp) = read_back(&d);
            assert_eq!(tags, vec![256, 257, 700, 50706]);
            assert!(same(&xmp, x1));
            assert!(same(&read(&p).unwrap().unwrap(), x1));
            assert!(!embed(&p, x1).unwrap());
            let x2 = "<x>2</x>\n<?xpacket end=\"w\"?>\n";
            assert!(embed(&p, x2).unwrap());
            let d2 = std::fs::read(&p).unwrap();
            assert_eq!(d2.len(), d.len(), "rewritten within the padding");
            assert!(same(&read_back(&d2).1, x2));
        }
    }

    #[test]
    fn replaces_small_existing_tag() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.tif");
        std::fs::write(&p, tiff(true, Some(b"<old/>"))).unwrap();
        let x = "<x>new</x>\n<?xpacket end=\"w\"?>\n";
        assert!(embed(&p, x).unwrap());
        let d = std::fs::read(&p).unwrap();
        let (tags, xmp) = read_back(&d);
        assert_eq!(tags, vec![256, 257, 700, 50706]);
        assert!(same(&xmp, x));
    }

    #[test]
    fn detects_raw_magics() {
        assert!(is_tiff(b"IIRO\x08\0\0\0"));
        assert!(is_tiff(b"IIU\0\x18\0\0\0"));
        assert!(is_tiff(b"MMOR\0\0\0\x08"));
        assert!(!is_tiff(b"II\0\x2b\0\0\0\0"), "BigTIFF is not handled");
    }
}
