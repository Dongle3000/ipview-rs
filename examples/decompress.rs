//! Writes the decompressed contents of a Unix-compressed (.Z) file to stdout.

use std::io::Write;

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).ok_or_else(|| anyhow::anyhow!("usage: decompress <file>"))?;
    let data = std::fs::read(&path)?;
    std::io::stdout().write_all(&ipview::compress::decompress(&data)?)?;
    Ok(())
}
