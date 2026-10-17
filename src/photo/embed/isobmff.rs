//! HEIC/HEIF store XMP as a `mime` item in the top-level `meta` box; growing
//! `meta` shifts everything behind it, so every `iloc` extent (and any `stco`
//! in an image-sequence `moov`) is rebased and the packet lands in a new `mdat`
//! appended to the file. MOV, MP4 and CR3 take a top-level `uuid` box appended
//! after the last box, which moves nothing; CR3 additionally records the box in
//! Canon's `CTBO` offset table.

use super::{pad, padded, read_at, same, tmp_path, write_at};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, TimeZone, Utc};
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

const XMP_UUID: [u8; 16] = [
    0xbe, 0x7a, 0xcf, 0xcb, 0x97, 0xa9, 0x42, 0xe8, 0x9c, 0x71, 0x99, 0x94, 0x91, 0xe3, 0xaf, 0xac,
];
const CANON_UUID: [u8; 16] = [
    0x85, 0xc0, 0xb6, 0x87, 0x82, 0x0f, 0x11, 0xe0, 0x81, 0x11, 0xf4, 0xce, 0x46, 0x2b, 0x6a, 0x48,
];
const RDF_MIME: &[u8] = b"application/rdf+xml\0";

#[derive(Clone, Debug)]
struct Atom {
    pos: u64,
    size: u64,
    kind: [u8; 4],
    header: u64,
}

impl Atom {
    fn body(&self) -> std::ops::Range<usize> {
        (self.pos + self.header) as usize..(self.pos + self.size) as usize
    }

    fn end(&self) -> u64 {
        self.pos + self.size
    }
}

struct Extent {
    index: u64,
    offset: u64,
    length: u64,
}

struct Item {
    id: u32,
    method: u8,
    dri: u16,
    base: u64,
    extents: Vec<Extent>,
}

struct Iloc {
    version: u8,
    flags: [u8; 3],
    offset_size: u8,
    length_size: u8,
    base_size: u8,
    index_size: u8,
    items: Vec<Item>,
}

fn be16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn be64(b: &[u8]) -> u64 {
    u64::from_be_bytes(b[..8].try_into().unwrap())
}

fn sized(b: &[u8], pos: &mut usize, size: u8) -> Result<u64> {
    let v = match size {
        0 => 0,
        4 => u64::from(be32(b.get(*pos..*pos + 4).context("truncated iloc")?)),
        8 => be64(b.get(*pos..*pos + 8).context("truncated iloc")?),
        n => bail!("iloc field size {n}"),
    };
    *pos += usize::from(size);
    Ok(v)
}

fn put_sized(out: &mut Vec<u8>, v: u64, size: u8) -> Result<()> {
    match size {
        0 => {}
        4 => out.extend_from_slice(
            &u32::try_from(v)
                .context("offset exceeds 32 bits")?
                .to_be_bytes(),
        ),
        8 => out.extend_from_slice(&v.to_be_bytes()),
        n => bail!("iloc field size {n}"),
    }
    Ok(())
}

fn parse_header(hdr: &[u8], pos: u64, end: u64) -> Result<Atom> {
    if hdr.len() < 8 {
        bail!("truncated box at {pos}");
    }
    let kind: [u8; 4] = hdr[4..8].try_into().unwrap();
    let (size, header) = match be32(hdr) {
        0 => (end - pos, 8),
        1 if hdr.len() >= 16 => (be64(&hdr[8..16]), 16),
        1 => bail!("truncated box at {pos}"),
        n => (u64::from(n), 8),
    };
    if size < header || pos + size > end {
        bail!(
            "bad {} box size {size} at {pos}",
            String::from_utf8_lossy(&kind)
        );
    }
    Ok(Atom {
        pos,
        size,
        kind,
        header,
    })
}

fn top_boxes(f: &mut File, len: u64) -> Result<Vec<Atom>> {
    let mut pos = 0;
    let mut out = Vec::new();
    while pos + 8 <= len {
        let hdr = read_at(f, pos, 16.min(len - pos) as usize)?;
        let b = parse_header(&hdr, pos, len)?;
        pos = b.end();
        out.push(b);
    }
    Ok(out)
}

fn boxes_in(buf: &[u8], start: usize, end: usize) -> Result<Vec<Atom>> {
    let mut pos = start;
    let mut out = Vec::new();
    while pos + 8 <= end {
        let b = parse_header(&buf[pos..end.min(pos + 16)], pos as u64, end as u64)?;
        pos = b.end() as usize;
        out.push(b);
    }
    Ok(out)
}

fn with_header(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 8);
    out.extend_from_slice(&((body.len() + 8) as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

pub fn is_isobmff(head: &[u8]) -> bool {
    matches!(
        head.get(4..8),
        Some(b"ftyp" | b"moov" | b"mdat" | b"wide" | b"free" | b"skip" | b"pnot")
    )
}

fn copy_range(f: &mut File, out: &mut File, pos: u64, len: u64) -> Result<()> {
    f.seek(SeekFrom::Start(pos))?;
    let n = std::io::copy(&mut f.take(len), out)?;
    if n != len {
        bail!("short read copying {len} bytes at {pos}");
    }
    Ok(())
}

trait Take {
    fn take(&mut self, n: u64) -> std::io::Take<&mut File>;
}

impl Take for File {
    fn take(&mut self, n: u64) -> std::io::Take<&mut File> {
        std::io::Read::take(self, n)
    }
}

/// The `mvhd` creation time of a MOV/MP4, if set.
pub fn creation_time(path: &Path) -> Option<DateTime<Utc>> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let moov = top_boxes(&mut f, len)
        .ok()?
        .into_iter()
        .find(|b| &b.kind == b"moov")?;
    let buf = read_at(&mut f, moov.pos, moov.size as usize).ok()?;
    let mvhd = boxes_in(&buf, moov.header as usize, buf.len())
        .ok()?
        .into_iter()
        .find(|b| &b.kind == b"mvhd")?;
    let b = &buf[mvhd.body()];
    let secs = if b.first()? == &0 {
        u64::from(be32(b.get(4..8)?))
    } else {
        be64(b.get(4..12)?)
    };
    if secs == 0 {
        return None;
    }
    let epoch = Utc.with_ymd_and_hms(1904, 1, 1, 0, 0, 0).single()?;
    epoch.checked_add_signed(chrono::Duration::seconds(i64::try_from(secs).ok()?))
}

/// The top-level `meta` box and its bytes, when the file is item-based (HEIF).
fn item_meta(f: &mut File, top: &[Atom]) -> Result<Option<(Atom, Vec<u8>)>> {
    let Some(meta) = top.iter().find(|b| &b.kind == b"meta") else {
        return Ok(None);
    };
    let buf = read_at(f, meta.pos, meta.size as usize)?;
    let body = (meta.header + 4) as usize..buf.len();
    let item_based = boxes_in(&buf, body.start, body.end)?
        .iter()
        .any(|b| &b.kind == b"iinf");
    Ok(item_based.then(|| (meta.clone(), buf)))
}

fn xmp_uuid(f: &mut File, top: &[Atom]) -> Result<Option<Atom>> {
    for b in top
        .iter()
        .filter(|b| &b.kind == b"uuid" && b.size >= b.header + 16)
    {
        if read_at(f, b.pos + b.header, 16)? == XMP_UUID {
            return Ok(Some(b.clone()));
        }
    }
    Ok(None)
}

pub fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let top = top_boxes(&mut f, len)?;
    if let Some((meta, buf)) = item_meta(&mut f, &top)? {
        let items = Items::parse(&meta, &buf)?;
        return match items.xmp_data() {
            Some((at, n)) => Ok(Some(read_at(&mut f, at, n)?)),
            None => Ok(None),
        };
    }
    match xmp_uuid(&mut f, &top)? {
        Some(b) => {
            let at = b.pos + b.header + 16;
            Ok(Some(read_at(&mut f, at, (b.end() - at) as usize)?))
        }
        None => Ok(None),
    }
}

pub fn embed(path: &Path, xmp: &str) -> Result<bool> {
    let mut f = OpenOptions::new().read(true).write(true).open(path)?;
    let len = f.metadata()?.len();
    let top = top_boxes(&mut f, len)?;
    let brand = match top.iter().find(|b| &b.kind == b"ftyp") {
        Some(b) if b.size >= b.header + 4 => read_at(&mut f, b.pos + b.header, 4)?,
        _ => b"qt  ".to_vec(),
    };
    if let Some((meta, buf)) = item_meta(&mut f, &top)? {
        return embed_item(path, &mut f, len, &top, &meta, buf, xmp);
    }
    embed_uuid(&mut f, len, &top, brand == b"crx ", xmp)
}

/// Absolute file position of the CTBO entry describing the XMP uuid box.
fn ctbo_entry(f: &mut File, top: &[Atom]) -> Result<Option<u64>> {
    let Some(moov) = top.iter().find(|b| &b.kind == b"moov") else {
        return Ok(None);
    };
    let buf = read_at(f, moov.pos, moov.size as usize)?;
    for child in boxes_in(&buf, moov.header as usize, buf.len())? {
        let body = child.body();
        if &child.kind != b"uuid" || buf.get(body.start..body.start + 16) != Some(&CANON_UUID) {
            continue;
        }
        for c in boxes_in(&buf, body.start + 16, body.end)? {
            if &c.kind != b"CTBO" {
                continue;
            }
            let b = c.body();
            let n = be32(&buf[b.start..]) as usize;
            for i in 0..n {
                let e = b.start + 4 + i * 20;
                if e + 20 > b.end {
                    bail!("truncated CTBO");
                }
                if be32(&buf[e..]) == 1 {
                    return Ok(Some(moov.pos + e as u64));
                }
            }
        }
    }
    Ok(None)
}

fn embed_uuid(f: &mut File, mut len: u64, top: &[Atom], cr3: bool, xmp: &str) -> Result<bool> {
    let ctbo = if cr3 {
        Some(ctbo_entry(f, top)?.context("CR3 without a CTBO entry for XMP")?)
    } else {
        None
    };
    if let Some(b) = xmp_uuid(f, top)? {
        let data_at = b.pos + b.header + 16;
        let data = read_at(f, data_at, (b.end() - data_at) as usize)?;
        if same(&data, xmp) {
            return Ok(false);
        }
        if data.len() >= xmp.len() {
            write_at(f, data_at, &pad(xmp, data.len()))?;
            return Ok(true);
        }
        if b.end() == len {
            f.set_len(b.pos)?;
            len = b.pos;
        } else {
            write_at(f, b.pos + 4, b"free")?;
        }
    }
    let mut body = XMP_UUID.to_vec();
    body.extend_from_slice(&padded(xmp));
    let atom = with_header(b"uuid", &body);
    write_at(f, len, &atom)?;
    if let Some(entry) = ctbo {
        write_at(f, entry + 4, &len.to_be_bytes())?;
        write_at(f, entry + 12, &(atom.len() as u64).to_be_bytes())?;
    }
    Ok(true)
}

fn parse_iloc(b: &[u8]) -> Result<Iloc> {
    let version = b[0];
    if version > 2 {
        bail!("iloc version {version}");
    }
    let offset_size = b[4] >> 4;
    let length_size = b[4] & 0xf;
    let base_size = b[5] >> 4;
    let index_size = if version == 0 { 0 } else { b[5] & 0xf };
    let mut p = 6;
    let count = if version < 2 {
        p += 2;
        be16(&b[6..]) as usize
    } else {
        p += 4;
        be32(&b[6..]) as usize
    };
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        let id = if version < 2 {
            p += 2;
            u32::from(be16(&b[p - 2..]))
        } else {
            p += 4;
            be32(&b[p - 4..])
        };
        let method = if version == 0 {
            0
        } else {
            p += 2;
            (be16(&b[p - 2..]) & 0xf) as u8
        };
        let dri = be16(b.get(p..p + 2).context("truncated iloc")?);
        p += 2;
        let base = sized(b, &mut p, base_size)?;
        let n = be16(b.get(p..p + 2).context("truncated iloc")?) as usize;
        p += 2;
        let mut extents = Vec::with_capacity(n);
        for _ in 0..n {
            let index = if version == 0 {
                0
            } else {
                sized(b, &mut p, index_size)?
            };
            let offset = sized(b, &mut p, offset_size)?;
            let length = sized(b, &mut p, length_size)?;
            extents.push(Extent {
                index,
                offset,
                length,
            });
        }
        items.push(Item {
            id,
            method,
            dri,
            base,
            extents,
        });
    }
    Ok(Iloc {
        version,
        flags: [b[1], b[2], b[3]],
        offset_size,
        length_size,
        base_size,
        index_size,
        items,
    })
}

fn write_iloc(iloc: &Iloc) -> Result<Vec<u8>> {
    let mut b = vec![iloc.version, iloc.flags[0], iloc.flags[1], iloc.flags[2]];
    b.push(iloc.offset_size << 4 | iloc.length_size);
    b.push(
        iloc.base_size << 4
            | if iloc.version == 0 {
                0
            } else {
                iloc.index_size
            },
    );
    if iloc.version < 2 {
        b.extend_from_slice(&u16::try_from(iloc.items.len())?.to_be_bytes());
    } else {
        b.extend_from_slice(&(iloc.items.len() as u32).to_be_bytes());
    }
    for it in &iloc.items {
        if iloc.version < 2 {
            b.extend_from_slice(&u16::try_from(it.id)?.to_be_bytes());
        } else {
            b.extend_from_slice(&it.id.to_be_bytes());
        }
        if iloc.version > 0 {
            b.extend_from_slice(&u16::from(it.method).to_be_bytes());
        }
        b.extend_from_slice(&it.dri.to_be_bytes());
        put_sized(&mut b, it.base, iloc.base_size)?;
        b.extend_from_slice(&u16::try_from(it.extents.len())?.to_be_bytes());
        for e in &it.extents {
            if iloc.version > 0 {
                put_sized(&mut b, e.index, iloc.index_size)?;
            }
            put_sized(&mut b, e.offset, iloc.offset_size)?;
            put_sized(&mut b, e.length, iloc.length_size)?;
        }
    }
    Ok(with_header(b"iloc", &b))
}

struct Infe {
    id: u32,
    xmp: bool,
}

fn parse_infe(b: &[u8]) -> Option<Infe> {
    let (id, type_at) = match b.first()? {
        2 => (u32::from(be16(b.get(4..6)?)), 8),
        3 => (be32(b.get(4..8)?), 10),
        _ => return None,
    };
    let kind = b.get(type_at..type_at + 4)?;
    let mut xmp = false;
    if kind == b"mime" {
        let rest = &b[type_at + 4..];
        let name_end = rest.iter().position(|c| *c == 0)?;
        xmp = rest[name_end + 1..].starts_with(RDF_MIME);
    }
    Some(Infe { id, xmp })
}

fn make_infe(id: u32) -> Vec<u8> {
    let mut b = Vec::new();
    if id < 0x10000 {
        b.extend_from_slice(&[2, 0, 0, 0]);
        b.extend_from_slice(&(id as u16).to_be_bytes());
    } else {
        b.extend_from_slice(&[3, 0, 0, 0]);
        b.extend_from_slice(&id.to_be_bytes());
    }
    b.extend_from_slice(&[0, 0]);
    b.extend_from_slice(b"mime\0");
    b.extend_from_slice(RDF_MIME);
    with_header(b"infe", &b)
}

fn make_cdsc(version: u8, from: u32, to: u32) -> Vec<u8> {
    let mut b = Vec::new();
    if version == 0 {
        b.extend_from_slice(&(from as u16).to_be_bytes());
        b.extend_from_slice(&1u16.to_be_bytes());
        b.extend_from_slice(&(to as u16).to_be_bytes());
    } else {
        b.extend_from_slice(&from.to_be_bytes());
        b.extend_from_slice(&1u16.to_be_bytes());
        b.extend_from_slice(&to.to_be_bytes());
    }
    with_header(b"cdsc", &b)
}

/// Adds `delta` to every `stco`/`co64` entry at or beyond `threshold`.
fn shift_chunk_offsets(
    buf: &mut [u8],
    start: usize,
    end: usize,
    threshold: u64,
    delta: i64,
) -> Result<()> {
    for b in boxes_in(buf, start, end)? {
        let body = b.body();
        match &b.kind {
            b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" => {
                shift_chunk_offsets(buf, body.start, body.end, threshold, delta)?
            }
            b"stco" | b"co64" => {
                let width = if &b.kind == b"stco" { 4 } else { 8 };
                let n = be32(&buf[body.start + 4..]) as usize;
                for i in 0..n {
                    let at = body.start + 8 + i * width;
                    let slot = buf
                        .get_mut(at..at + width)
                        .context("truncated chunk offsets")?;
                    let v = if width == 4 {
                        u64::from(be32(slot))
                    } else {
                        be64(slot)
                    };
                    if v < threshold {
                        continue;
                    }
                    let v = (v as i64 + delta) as u64;
                    if width == 4 {
                        slot.copy_from_slice(&u32::try_from(v)?.to_be_bytes());
                    } else {
                        slot.copy_from_slice(&v.to_be_bytes());
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// The parts of a `meta` box that item insertion touches.
struct Items {
    children: Vec<Atom>,
    primary: u32,
    count_len: usize,
    max_id: u32,
    xmp_id: Option<u32>,
    iloc: Iloc,
    idat: Option<u64>,
}

impl Items {
    fn parse(meta: &Atom, buf: &[u8]) -> Result<Items> {
        let body_start = (meta.header + 4) as usize;
        let children = boxes_in(buf, body_start, buf.len())?;
        let child = |k: &[u8; 4]| children.iter().find(|b| &b.kind == k);
        let pitm = child(b"pitm").context("no pitm")?.body();
        let primary = if buf[pitm.start] == 0 {
            u32::from(be16(&buf[pitm.start + 4..]))
        } else {
            be32(&buf[pitm.start + 4..])
        };
        let iinf = child(b"iinf").context("no iinf")?.body();
        let count_len = if buf[iinf.start] == 0 { 2 } else { 4 };
        let mut max_id = 0;
        let mut xmp_id = None;
        for inf in boxes_in(buf, iinf.start + 4 + count_len, iinf.end)? {
            if let Some(i) = parse_infe(&buf[inf.body()]) {
                max_id = max_id.max(i.id);
                if i.xmp && xmp_id.is_none() {
                    xmp_id = Some(i.id);
                }
            }
        }
        let iloc = parse_iloc(&buf[child(b"iloc").context("no iloc")?.body()])?;
        max_id = iloc.items.iter().fold(max_id, |m, it| m.max(it.id));
        let idat = child(b"idat").map(|b| meta.pos + b.body().start as u64);
        Ok(Items {
            children,
            primary,
            count_len,
            max_id,
            xmp_id,
            iloc,
            idat,
        })
    }

    /// File position and length of the existing XMP item's data.
    fn xmp_data(&self) -> Option<(u64, usize)> {
        let it = self
            .iloc
            .items
            .iter()
            .find(|it| Some(it.id) == self.xmp_id)?;
        let [e] = it.extents.as_slice() else {
            return None;
        };
        let at = match it.method {
            0 => it.base + e.offset,
            1 => self.idat? + it.base + e.offset,
            _ => return None,
        };
        Some((at, e.length as usize))
    }
}

fn embed_item(
    path: &Path,
    f: &mut File,
    len: u64,
    top: &[Atom],
    meta: &Atom,
    buf: Vec<u8>,
    xmp: &str,
) -> Result<bool> {
    let items = Items::parse(meta, &buf)?;
    if let Some((at, n)) = items.xmp_data() {
        let existing = read_at(f, at, n)?;
        if same(&existing, xmp) {
            return Ok(false);
        }
        if existing.len() >= xmp.len() {
            write_at(f, at, &pad(xmp, existing.len()))?;
            return Ok(true);
        }
    }
    let Items {
        children,
        primary,
        count_len,
        max_id,
        xmp_id,
        mut iloc,
        ..
    } = items;
    let body_start = (meta.header + 4) as usize;
    let child = |k: &[u8; 4]| children.iter().find(|b| &b.kind == k);

    let id = xmp_id.unwrap_or(max_id + 1);
    let payload = padded(xmp);
    if iloc.offset_size == 0 {
        iloc.offset_size = 4;
    }
    if iloc.length_size == 0 {
        iloc.length_size = 4;
    }
    if len + payload.len() as u64 + (buf.len() as u64) > u64::from(u32::MAX) {
        iloc.offset_size = 8;
    }
    let new_item = Item {
        id,
        method: 0,
        dri: 0,
        base: 0,
        extents: vec![Extent {
            index: 0,
            offset: 0,
            length: payload.len() as u64,
        }],
    };
    match iloc.items.iter_mut().find(|it| it.id == id) {
        Some(it) => *it = new_item,
        None => iloc.items.push(new_item),
    }
    let new_infe = xmp_id.is_none().then(|| make_infe(id));
    let iref_version = child(b"iref")
        .map(|b| buf[b.body().start])
        .unwrap_or(u8::from(primary >= 0x10000 || id >= 0x10000));
    let cdsc = new_infe
        .as_ref()
        .map(|_| make_cdsc(iref_version, id, primary));

    let mut iloc_len = write_iloc(&iloc)?.len() as u64;
    let mut new_meta_len = meta.header + 4;
    for c in &children {
        new_meta_len += match &c.kind {
            b"iloc" => iloc_len,
            b"iinf" => c.size + new_infe.as_ref().map_or(0, |i| i.len() as u64),
            b"iref" => c.size + cdsc.as_ref().map_or(0, |c| c.len() as u64),
            _ => c.size,
        };
    }
    if child(b"iref").is_none()
        && let Some(c) = &cdsc
    {
        new_meta_len += 12 + c.len() as u64;
    }
    let delta = new_meta_len as i64 - meta.size as i64;
    let threshold = meta.end();
    for it in iloc
        .items
        .iter_mut()
        .filter(|it| it.id != id && it.method == 0)
    {
        if it.base != 0 {
            if it.base >= threshold {
                it.base = (it.base as i64 + delta) as u64;
            }
        } else {
            for e in it.extents.iter_mut().filter(|e| e.offset >= threshold) {
                e.offset = (e.offset as i64 + delta) as u64;
            }
        }
    }
    let new_len = (len as i64 + delta) as u64;
    iloc.items
        .iter_mut()
        .find(|it| it.id == id)
        .unwrap()
        .extents[0]
        .offset = new_len + 8;
    let iloc_bytes = write_iloc(&iloc)?;
    iloc_len = iloc_bytes.len() as u64;
    debug_assert_eq!(iloc_len, write_iloc(&iloc)?.len() as u64);

    let mut out = Vec::with_capacity(new_meta_len as usize);
    if meta.header == 16 {
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(b"meta");
        out.extend_from_slice(&new_meta_len.to_be_bytes());
    } else {
        out.extend_from_slice(&u32::try_from(new_meta_len)?.to_be_bytes());
        out.extend_from_slice(b"meta");
    }
    out.extend_from_slice(&buf[meta.header as usize..body_start]);
    for c in &children {
        let range = c.pos as usize..c.end() as usize;
        match &c.kind {
            b"iloc" => out.extend_from_slice(&iloc_bytes),
            b"iinf" => {
                let Some(infe) = &new_infe else {
                    out.extend_from_slice(&buf[range]);
                    continue;
                };
                let b = c.body();
                let mut body = buf[b.clone()].to_vec();
                if count_len == 2 {
                    let n = be16(&body[4..]) + 1;
                    body[4..6].copy_from_slice(&n.to_be_bytes());
                } else {
                    let n = be32(&body[4..]) + 1;
                    body[4..8].copy_from_slice(&n.to_be_bytes());
                }
                body.extend_from_slice(infe);
                out.extend_from_slice(&with_header(b"iinf", &body));
                if child(b"iref").is_none() {
                    let mut iref = vec![iref_version, 0, 0, 0];
                    iref.extend_from_slice(cdsc.as_ref().unwrap());
                    out.extend_from_slice(&with_header(b"iref", &iref));
                }
            }
            b"iref" => match &cdsc {
                Some(cdsc) => {
                    let mut body = buf[c.body()].to_vec();
                    body.extend_from_slice(cdsc);
                    out.extend_from_slice(&with_header(b"iref", &body));
                }
                None => out.extend_from_slice(&buf[range]),
            },
            _ => out.extend_from_slice(&buf[range]),
        }
    }
    if out.len() as u64 != new_meta_len {
        bail!("meta size mismatch: {} vs {new_meta_len}", out.len());
    }

    let tmp = tmp_path(path);
    let mut w = File::create(&tmp)?;
    copy_range(f, &mut w, 0, meta.pos)?;
    w.write_all(&out)?;
    for b in top.iter().filter(|b| b.pos >= meta.end()) {
        if &b.kind == b"moov" && delta != 0 {
            let mut moov = read_at(f, b.pos, b.size as usize)?;
            let n = moov.len();
            shift_chunk_offsets(&mut moov, 0, n, threshold, delta)?;
            w.write_all(&moov)?;
        } else {
            copy_range(f, &mut w, b.pos, b.size)?;
        }
    }
    w.write_all(&with_header(b"mdat", &payload))?;
    w.flush()?;
    drop(w);
    std::fs::rename(&tmp, path)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fullbox(kind: &[u8; 4], version: u8, body: &[u8]) -> Vec<u8> {
        let mut b = vec![version, 0, 0, 0];
        b.extend_from_slice(body);
        with_header(kind, &b)
    }

    fn ftyp(brand: &[u8; 4]) -> Vec<u8> {
        let mut b = brand.to_vec();
        b.extend_from_slice(&[0, 0, 0, 0]);
        b.extend_from_slice(b"mif1");
        with_header(b"ftyp", &b)
    }

    /// Minimal HEIC: one hvc1 item whose data lives in mdat.
    fn heic(iloc_version: u8) -> Vec<u8> {
        let hdlr = fullbox(
            b"hdlr",
            0,
            &[&[0u8; 4][..], b"pict", &[0u8; 12], &[0]].concat(),
        );
        let pitm = fullbox(b"pitm", 0, &1u16.to_be_bytes());
        let infe = fullbox(
            b"infe",
            2,
            &[&1u16.to_be_bytes()[..], &[0, 0], b"hvc1", &[0]].concat(),
        );
        let iinf = fullbox(b"iinf", 0, &[&1u16.to_be_bytes()[..], &infe].concat());
        let iprp = with_header(b"iprp", &with_header(b"ipco", &[]));
        let iloc_len = if iloc_version == 0 { 30 } else { 32 };
        let meta_len = 8 + 4 + hdlr.len() + pitm.len() + iinf.len() + iprp.len() + iloc_len;
        let mdat_at = ftyp(b"heic").len() + meta_len;
        let mut il = vec![0x44, 0x00];
        il.extend_from_slice(&1u16.to_be_bytes());
        il.extend_from_slice(&1u16.to_be_bytes());
        if iloc_version > 0 {
            il.extend_from_slice(&0u16.to_be_bytes());
        }
        il.extend_from_slice(&0u16.to_be_bytes());
        il.extend_from_slice(&1u16.to_be_bytes());
        il.extend_from_slice(&((mdat_at + 8) as u32).to_be_bytes());
        il.extend_from_slice(&5u32.to_be_bytes());
        let iloc = fullbox(b"iloc", iloc_version, &il);
        assert_eq!(iloc.len(), iloc_len);
        let meta = fullbox(b"meta", 0, &[hdlr, pitm, iinf, iprp, iloc].concat());
        [ftyp(b"heic"), meta, with_header(b"mdat", b"IMAGE")].concat()
    }

    struct Parsed {
        items: Vec<(u32, Vec<u8>)>,
        infe_count: u16,
        cdsc: Vec<(u32, u32)>,
    }

    fn parse_heic(d: &[u8]) -> Parsed {
        let top = boxes_in(d, 0, d.len()).unwrap();
        let meta = top.iter().find(|b| &b.kind == b"meta").unwrap();
        let children = boxes_in(d, meta.body().start + 4, meta.body().end).unwrap();
        let iloc = children.iter().find(|b| &b.kind == b"iloc").unwrap();
        let iloc = parse_iloc(&d[iloc.body()]).unwrap();
        let items = iloc
            .items
            .iter()
            .map(|it| {
                let e = &it.extents[0];
                let at = (it.base + e.offset) as usize;
                (it.id, d[at..at + e.length as usize].to_vec())
            })
            .collect();
        let iinf = children.iter().find(|b| &b.kind == b"iinf").unwrap();
        let infe_count = be16(&d[iinf.body().start + 4..]);
        let cdsc = children
            .iter()
            .find(|b| &b.kind == b"iref")
            .map(|iref| {
                boxes_in(d, iref.body().start + 4, iref.body().end)
                    .unwrap()
                    .iter()
                    .map(|c| {
                        (
                            u32::from(be16(&d[c.body()])),
                            u32::from(be16(&d[c.body().start + 4..])),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Parsed {
            items,
            infe_count,
            cdsc,
        }
    }

    #[test]
    fn heic_adds_item_and_rebases_extents() {
        for v in [0u8, 1] {
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path().join("a.heic");
            std::fs::write(&p, heic(v)).unwrap();
            let x1 = "<x>1</x>\n<?xpacket end=\"w\"?>\n";
            assert!(embed(&p, x1).unwrap());
            let d = std::fs::read(&p).unwrap();
            let parsed = parse_heic(&d);
            assert_eq!(parsed.infe_count, 2);
            assert_eq!(parsed.cdsc, vec![(2, 1)]);
            assert_eq!(
                parsed.items[0],
                (1, b"IMAGE".to_vec()),
                "image extent follows the grown meta"
            );
            assert_eq!(parsed.items[1].0, 2);
            assert!(same(&parsed.items[1].1, x1));
            assert!(same(&read(&p).unwrap().unwrap(), x1));
            assert!(!embed(&p, x1).unwrap());

            let x2 = "<x>2</x>\n<?xpacket end=\"w\"?>\n";
            assert!(embed(&p, x2).unwrap());
            let d2 = std::fs::read(&p).unwrap();
            assert_eq!(d2.len(), d.len(), "fits in padding");
            assert!(same(&parse_heic(&d2).items[1].1, x2));

            let x3 = format!("<x>{}</x>\n<?xpacket end=\"w\"?>\n", "3".repeat(4000));
            assert!(embed(&p, &x3).unwrap());
            let d3 = std::fs::read(&p).unwrap();
            let parsed = parse_heic(&d3);
            assert_eq!(parsed.infe_count, 2, "existing item reused");
            assert_eq!(parsed.cdsc, vec![(2, 1)]);
            assert_eq!(parsed.items[0], (1, b"IMAGE".to_vec()));
            assert!(same(&parsed.items[1].1, &x3));
        }
    }

    fn mp4(brand: &[u8; 4], extra: Vec<u8>) -> Vec<u8> {
        let moov = with_header(b"moov", &fullbox(b"mvhd", 0, &[0u8; 96]));
        [ftyp(brand), moov, extra, with_header(b"mdat", b"VIDEO")].concat()
    }

    fn xmp_boxes(f: &mut File) -> Vec<(u64, Vec<u8>)> {
        let len = f.metadata().unwrap().len();
        top_boxes(f, len)
            .unwrap()
            .into_iter()
            .filter(|b| &b.kind == b"uuid")
            .filter_map(|b| {
                let body = read_at(f, b.pos + 8, (b.size - 8) as usize).unwrap();
                (body[..16] == XMP_UUID).then(|| (b.pos, body[16..].to_vec()))
            })
            .collect()
    }

    #[test]
    fn mp4_appends_then_rewrites_in_place_then_reappends() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.mp4");
        let orig = mp4(b"isom", Vec::new());
        std::fs::write(&p, &orig).unwrap();
        let x1 = "<x>1</x>\n<?xpacket end=\"w\"?>\n";
        assert!(embed(&p, x1).unwrap());
        let mut f = File::open(&p).unwrap();
        let boxes = xmp_boxes(&mut f);
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].0 as usize, orig.len());
        assert!(same(&boxes[0].1, x1));
        assert!(std::fs::read(&p).unwrap().starts_with(&orig));
        assert!(same(&read(&p).unwrap().unwrap(), x1));
        assert!(!embed(&p, x1).unwrap());
        let len1 = f.metadata().unwrap().len();
        assert!(embed(&p, "<x>2</x>\n<?xpacket end=\"w\"?>\n").unwrap());
        assert_eq!(f.metadata().unwrap().len(), len1);
        let x3 = format!("<x>{}</x>\n<?xpacket end=\"w\"?>\n", "3".repeat(4000));
        assert!(embed(&p, &x3).unwrap());
        let boxes = xmp_boxes(&mut f);
        assert_eq!(boxes.len(), 1, "last box was truncated, not freed");
        assert_eq!(boxes[0].0 as usize, orig.len());
        assert!(same(&boxes[0].1, &x3));
    }

    #[test]
    fn frees_old_box_that_is_not_last() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.mov");
        let old = with_header(b"uuid", &[&XMP_UUID[..], b"<old/>"].concat());
        let orig = mp4(b"qt  ", old);
        std::fs::write(&p, &orig).unwrap();
        let x = "<x>new and longer</x>\n<?xpacket end=\"w\"?>\n";
        assert!(embed(&p, x).unwrap());
        let mut f = File::open(&p).unwrap();
        let boxes = xmp_boxes(&mut f);
        assert_eq!(boxes.len(), 1);
        assert!(same(&boxes[0].1, x));
        let d = std::fs::read(&p).unwrap();
        let top = boxes_in(&d, 0, d.len()).unwrap();
        assert_eq!(&top[2].kind, b"free");
        assert_eq!(&top[3].kind, b"mdat");
    }

    fn cr3(xmp: Option<&[u8]>) -> Vec<u8> {
        let mut ctbo = 4u32.to_be_bytes().to_vec();
        for id in 1..=4u32 {
            ctbo.extend_from_slice(&id.to_be_bytes());
            ctbo.extend_from_slice(&0u64.to_be_bytes());
            ctbo.extend_from_slice(&0u64.to_be_bytes());
        }
        let canon = with_header(
            b"uuid",
            &[&CANON_UUID[..], &with_header(b"CTBO", &ctbo)].concat(),
        );
        let moov = with_header(b"moov", &[fullbox(b"mvhd", 0, &[0u8; 96]), canon].concat());
        let old = xmp
            .map(|x| with_header(b"uuid", &[&XMP_UUID[..], x].concat()))
            .unwrap_or_default();
        [ftyp(b"crx "), moov, old, with_header(b"mdat", b"RAW")].concat()
    }

    fn ctbo_xmp(d: &[u8]) -> (u64, u64) {
        let at = super::super::find(d, b"CTBO").unwrap() + 4 + 4;
        (be64(&d[at + 4..]), be64(&d[at + 12..]))
    }

    #[test]
    fn cr3_updates_ctbo() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.cr3");
        let orig = cr3(None);
        std::fs::write(&p, &orig).unwrap();
        let x = "<x>1</x>\n<?xpacket end=\"w\"?>\n";
        assert!(embed(&p, x).unwrap());
        let d = std::fs::read(&p).unwrap();
        let (off, size) = ctbo_xmp(&d);
        assert_eq!(off as usize, orig.len());
        assert_eq!(off + size, d.len() as u64);
        assert_eq!(&d[off as usize + 4..off as usize + 8], b"uuid");

        std::fs::write(&p, cr3(Some(b"<old/>"))).unwrap();
        assert!(embed(&p, x).unwrap());
        let d = std::fs::read(&p).unwrap();
        let (off, size) = ctbo_xmp(&d);
        assert_eq!(off + size, d.len() as u64);
        let top = boxes_in(&d, 0, d.len()).unwrap();
        assert_eq!(&top[2].kind, b"free");
        let mut f = File::open(&p).unwrap();
        assert_eq!(xmp_boxes(&mut f).len(), 1);
    }
}
