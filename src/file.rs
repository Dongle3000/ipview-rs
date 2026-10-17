//! Reading and writing ticket files.
//!
//! * `IP*.DAT` files as collected from the OXE, usually Unix-compressed
//!   (magic `1F 9D`), sometimes plain.
//! * `.ipv` files, IPView's own save format: the raw ticket records of the
//!   selected tickets concatenated, i.e. an uncompressed ticket stream.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::compress;
use crate::ticket::{self, Ticket};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Compressed,
    Plain,
}

/// Detects the container format from the first bytes, like IPView did:
/// `1F 9D` is compressed, `01 00` (field 1, the first entry of a ticket) is plain.
pub fn detect(data: &[u8]) -> Option<Format> {
    match data {
        [0x1f, 0x9d, ..] => Some(Format::Compressed),
        [0x01, 0x00, ..] => Some(Format::Plain),
        _ => None,
    }
}

pub fn parse(data: &[u8]) -> Result<Vec<Ticket>> {
    match detect(data) {
        Some(Format::Compressed) => Ok(ticket::split(&compress::decompress(data)?)),
        Some(Format::Plain) => Ok(ticket::split(data)),
        None if data.is_empty() => Ok(Vec::new()),
        None => bail!("not a ticket file (starts with {:02x?})", &data[..data.len().min(2)]),
    }
}

pub fn read(path: &Path) -> Result<Vec<Ticket>> {
    let data = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    parse(&data).with_context(|| format!("parsing {}", path.display()))
}

/// Whether a directory entry looks like a ticket file: `IP*.DAT` or `*.ipv`.
pub fn is_ticket_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    (lower.starts_with("ip") && lower.ends_with(".dat")) || lower.ends_with(".ipv")
}

/// Expands directories into the ticket files they contain (non-recursive, sorted).
pub fn expand(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for path in paths {
        if path.is_dir() {
            let mut found: Vec<PathBuf> = fs::read_dir(path)
                .with_context(|| format!("listing {}", path.display()))?
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .filter(|e| is_ticket_file_name(&e.file_name().to_string_lossy()))
                .map(|e| e.path())
                .collect();
            found.sort();
            out.extend(found);
        } else {
            out.push(path.clone());
        }
    }
    Ok(out)
}

/// Writes tickets as an `.ipv` file that IPView 2.0.1 can open.
pub fn write_ipv<'a>(path: &Path, tickets: impl IntoIterator<Item = &'a Ticket>) -> Result<usize> {
    let mut out =
        std::io::BufWriter::new(fs::File::create(path).with_context(|| format!("creating {}", path.display()))?);
    let mut n = 0;
    for t in tickets {
        out.write_all(t.raw())?;
        n += 1;
    }
    out.flush()?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compress::tests::compress;
    use crate::ticket::tests::tlv;

    fn stream() -> Vec<u8> {
        let mut s = tlv(&[(1, &[0, 0, 0, 1]), (12, &[0, 5])]);
        s.extend(tlv(&[(1, &[0, 0, 0, 2]), (12, &[0, 6])]));
        s
    }

    #[test]
    fn parses_plain_and_compressed() {
        let plain = stream();
        assert_eq!(parse(&plain).unwrap().len(), 2);
        assert_eq!(parse(&compress(&plain, 16)).unwrap(), parse(&plain).unwrap());
    }

    #[test]
    fn rejects_unknown_content() {
        assert!(parse(b"hello").is_err());
        assert!(parse(b"").unwrap().is_empty());
    }

    #[test]
    fn recognizes_file_names() {
        assert!(is_ticket_file_name("IPBANHI.DAT"));
        assert!(is_ticket_file_name("ip0001.dat"));
        assert!(is_ticket_file_name("new.ipv"));
        assert!(!is_ticket_file_name("readme.txt"));
        assert!(!is_ticket_file_name("TAX.DAT"));
    }
}
