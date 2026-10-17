//! Writes the XMP packet produced by [`super::xmp::write`] into the media file
//! itself, in the container's native slot: an APP1 segment for JPEG, an `iTXt`
//! chunk for PNG, IFD0 tag 700 for TIFF-based RAW and DNG, a `mime` item for
//! HEIC/HEIF, and a top-level `uuid` box for MOV, MP4 and CR3. Existing packets
//! are compared first and rewritten in place when the new one fits.

mod isobmff;
mod jpeg;
mod png;
mod tiff;

pub use isobmff::creation_time;

use anyhow::Result;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const PAD: usize = 2048;
const XPACKET_END: &[u8] = b"<?xpacket end";

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Embed {
    Written,
    Unchanged,
    Unsupported,
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The packet with trailing padding whitespace removed, for comparison.
fn stripped(packet: &[u8]) -> &[u8] {
    let head = match find(packet, XPACKET_END) {
        Some(i) => &packet[..i],
        None => packet,
    };
    let end = head
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(0, |i| i + 1);
    &head[..end]
}

fn same(existing: &[u8], xmp: &str) -> bool {
    stripped(existing) == stripped(xmp.as_bytes())
}

/// The packet padded with whitespace before the trailer so it is exactly
/// `len` bytes long, or unchanged when it is already longer.
fn pad(xmp: &str, len: usize) -> Vec<u8> {
    let bytes = xmp.as_bytes();
    let extra = len.saturating_sub(bytes.len());
    let at = find(bytes, XPACKET_END).unwrap_or(bytes.len());
    let mut out = Vec::with_capacity(bytes.len() + extra);
    out.extend_from_slice(&bytes[..at]);
    for i in 0..extra {
        out.push(if i % 100 == 99 { b'\n' } else { b' ' });
    }
    out.extend_from_slice(&bytes[at..]);
    out
}

fn padded(xmp: &str) -> Vec<u8> {
    pad(xmp, xmp.len() + PAD)
}

fn read_at(f: &mut File, pos: u64, len: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    f.seek(SeekFrom::Start(pos))?;
    f.read_exact(&mut buf)?;
    Ok(buf)
}

fn write_at(f: &mut File, pos: u64, data: &[u8]) -> Result<()> {
    f.seek(SeekFrom::Start(pos))?;
    f.write_all(data)?;
    Ok(())
}

/// Writes `data` to a temporary file beside `path` and renames it into place.
fn replace_file(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = tmp_path(path);
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn tmp_path(path: &Path) -> std::path::PathBuf {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    path.with_file_name(format!(".{name}.embed"))
}

#[derive(Clone, Copy)]
enum Format {
    Jpeg,
    Png,
    Tiff,
    Isobmff,
}

fn sniff(path: &Path) -> Result<Option<Format>> {
    let mut head = [0u8; 12];
    let n = File::open(path)?.read(&mut head)?;
    let head = &head[..n];
    Ok(if head.starts_with(&[0xff, 0xd8]) {
        Some(Format::Jpeg)
    } else if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Format::Png)
    } else if tiff::is_tiff(head) {
        Some(Format::Tiff)
    } else if isobmff::is_isobmff(head) {
        Some(Format::Isobmff)
    } else {
        None
    })
}

/// The XMP packet currently embedded in the file, if any.
pub fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    match sniff(path)? {
        Some(Format::Jpeg) => jpeg::read(path),
        Some(Format::Png) => png::read(path),
        Some(Format::Tiff) => tiff::read(path),
        Some(Format::Isobmff) => isobmff::read(path),
        None => Ok(None),
    }
}

pub fn embed(path: &Path, xmp: &str) -> Result<Embed> {
    let written = match sniff(path)? {
        Some(Format::Jpeg) => jpeg::embed(path, xmp)?,
        Some(Format::Png) => png::embed(path, xmp)?,
        Some(Format::Tiff) => tiff::embed(path, xmp)?,
        Some(Format::Isobmff) => isobmff::embed(path, xmp)?,
        None => return Ok(Embed::Unsupported),
    };
    Ok(if written {
        Embed::Written
    } else {
        Embed::Unchanged
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_before_trailer_and_compares_without_padding() {
        let xmp = "<x>a</x>\n<?xpacket end=\"w\"?>\n";
        let p = pad(xmp, 300);
        assert_eq!(p.len(), 300);
        assert!(p.ends_with(b"<?xpacket end=\"w\"?>\n"));
        assert!(same(&p, xmp));
        assert!(!same(&p, "<x>b</x>\n<?xpacket end=\"w\"?>\n"));
        assert_eq!(pad(xmp, 3).len(), xmp.len());
    }

    #[test]
    fn unknown_format_is_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.bin");
        std::fs::write(&p, b"hello world, nothing to see").unwrap();
        assert_eq!(embed(&p, "<x/>").unwrap(), Embed::Unsupported);
        assert_eq!(read(&p).unwrap(), None);
    }
}
