//! Decoder for the Unix `compress` (.Z) format.
//!
//! OXE ticket files are usually stored compressed with `compress`, which
//! IPView 2.0.1 detected by the `1F 9D` magic and decompressed into a temp
//! file before parsing. This is a port of the decompression half of
//! ncompress, including its quirk of discarding the remainder of a code
//! group whenever the code width changes or the table is cleared.

use std::fmt;

pub const MAGIC: [u8; 2] = [0x1f, 0x9d];

const BLOCK_MODE: u8 = 0x80;
const BITS_MASK: u8 = 0x1f;
const INIT_BITS: u32 = 9;
const CLEAR: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    NotCompressed,
    UnsupportedBits(u8),
    Corrupt { code: usize, free_ent: usize, bytes_in: usize, bytes_out: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotCompressed => write!(f, "input not in compress format"),
            Error::UnsupportedBits(b) => write!(f, "compressed with {b} bits, but can only handle 9..=16 bits"),
            Error::Corrupt { code, free_ent, bytes_in, bytes_out } => {
                write!(f, "corrupt input: code={code}, free_ent={free_ent}, bytes in={bytes_in}, bytes out={bytes_out}")
            }
        }
    }
}

impl std::error::Error for Error {}

pub fn is_compressed(data: &[u8]) -> bool {
    data.starts_with(&MAGIC)
}

pub fn decompress(data: &[u8]) -> Result<Vec<u8>, Error> {
    if data.len() < 3 || !is_compressed(data) {
        return Err(Error::NotCompressed);
    }
    let flags = data[2];
    let max_bits = (flags & BITS_MASK) as u32;
    if !(INIT_BITS..=16).contains(&max_bits) {
        return Err(Error::UnsupportedBits(flags & BITS_MASK));
    }
    let block_mode = flags & BLOCK_MODE != 0;
    let input = &data[3..];
    let total_bits = input.len() * 8;

    let max_max_code = 1usize << max_bits;
    let mut prefix = vec![0u16; max_max_code];
    let mut suffix = vec![0u8; max_max_code];
    for (i, s) in suffix.iter_mut().enumerate().take(256) {
        *s = i as u8;
    }

    let max_code_for = |n_bits: u32| {
        if n_bits == max_bits { max_max_code } else { (1usize << n_bits) - 1 }
    };

    let mut n_bits = INIT_BITS;
    let mut max_code = max_code_for(n_bits);
    let mut free_ent = if block_mode { CLEAR + 1 } else { CLEAR };
    // Codes are packed in groups of `n_bits` bytes (8 codes). Alignment to the
    // next group boundary is relative to where the current width started.
    let mut group_start = 0usize;
    let mut pos = 0usize;

    let mut out = Vec::with_capacity(input.len() * 3);
    let mut stack = Vec::with_capacity(max_max_code);
    let mut old_code: Option<usize> = None;
    let mut fin_char = 0u8;

    let align = |pos: usize, group_start: usize, n_bits: u32| {
        let group = (n_bits as usize) * 8;
        let rel = pos - group_start;
        group_start + rel.div_ceil(group) * group
    };

    while pos + n_bits as usize <= total_bits {
        if free_ent > max_code {
            pos = align(pos, group_start, n_bits);
            group_start = pos;
            n_bits += 1;
            max_code = max_code_for(n_bits);
            continue;
        }

        let code = read_code(input, pos, n_bits);
        pos += n_bits as usize;

        let Some(prev) = old_code else {
            if code >= 256 {
                return Err(corrupt(code, free_ent, pos, out.len()));
            }
            fin_char = code as u8;
            out.push(fin_char);
            old_code = Some(code);
            continue;
        };

        if code == CLEAR && block_mode {
            prefix[..256].fill(0);
            free_ent = CLEAR;
            pos = align(pos, group_start, n_bits);
            group_start = pos;
            n_bits = INIT_BITS;
            max_code = max_code_for(n_bits);
            continue;
        }

        let in_code = code;
        let mut c = code;
        stack.clear();
        if c >= free_ent {
            // KwKwK case: the code being defined right now.
            if c > free_ent {
                return Err(corrupt(code, free_ent, pos, out.len()));
            }
            stack.push(fin_char);
            c = prev;
        }
        while c >= 256 {
            stack.push(suffix[c]);
            c = prefix[c] as usize;
        }
        fin_char = suffix[c];
        stack.push(fin_char);
        out.extend(stack.iter().rev());

        if free_ent < max_max_code {
            prefix[free_ent] = prev as u16;
            suffix[free_ent] = fin_char;
            free_ent += 1;
        }
        old_code = Some(in_code);
    }

    Ok(out)
}

fn read_code(input: &[u8], pos: usize, n_bits: u32) -> usize {
    let byte = pos / 8;
    let mut word = 0u32;
    for i in 0..3 {
        if let Some(&b) = input.get(byte + i) {
            word |= (b as u32) << (8 * i);
        }
    }
    ((word >> (pos % 8)) & ((1 << n_bits) - 1)) as usize
}

fn corrupt(code: usize, free_ent: usize, pos: usize, bytes_out: usize) -> Error {
    Error::Corrupt { code, free_ent, bytes_in: pos / 8 + 3, bytes_out }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Minimal ncompress-compatible encoder, used only to round-trip test the decoder.
    pub(crate) fn compress(data: &[u8], max_bits: u32) -> Vec<u8> {
        let max_max_code = 1usize << max_bits;
        let mut w = BitWriter::default();
        let mut n_bits = INIT_BITS;
        let mut dict: HashMap<(usize, u8), usize> = HashMap::new();
        let mut free_ent = CLEAR + 1;

        let mut iter = data.iter();
        if let Some(&first) = iter.next() {
            let mut ent = first as usize;
            for &c in iter {
                if let Some(&code) = dict.get(&(ent, c)) {
                    ent = code;
                    continue;
                }
                w.write(ent, n_bits);
                if free_ent < max_max_code {
                    dict.insert((ent, c), free_ent);
                    free_ent += 1;
                }
                ent = c as usize;
                // The encoder runs one table entry ahead of the decoder, so it
                // widens one step later than the decoder's `free_ent > max_code`.
                if n_bits < max_bits && free_ent > (1 << n_bits) {
                    w.pad_group(n_bits);
                    n_bits += 1;
                }
            }
            w.write(ent, n_bits);
        }

        let mut out = vec![MAGIC[0], MAGIC[1], BLOCK_MODE | max_bits as u8];
        out.extend(w.finish());
        out
    }

    #[derive(Default)]
    struct BitWriter {
        bytes: Vec<u8>,
        bits: usize,
        group_start: usize,
    }

    impl BitWriter {
        fn write(&mut self, code: usize, n_bits: u32) {
            for i in 0..n_bits as usize {
                if self.bits.is_multiple_of(8) {
                    self.bytes.push(0);
                }
                if code >> i & 1 == 1 {
                    *self.bytes.last_mut().unwrap() |= 1 << (self.bits % 8);
                }
                self.bits += 1;
            }
        }

        fn pad_group(&mut self, n_bits: u32) {
            let group = n_bits as usize * 8;
            let rel = self.bits - self.group_start;
            let target = self.group_start + rel.div_ceil(group) * group;
            while self.bits < target {
                self.write(0, 1);
            }
            self.group_start = self.bits;
        }

        fn finish(self) -> Vec<u8> {
            self.bytes
        }
    }

    #[test]
    fn rejects_plain_data() {
        assert_eq!(decompress(b"\x01\x00\x04abcd"), Err(Error::NotCompressed));
    }

    #[test]
    fn round_trips_small_input() {
        let data = b"TOBEORNOTTOBEORTOBEORNOT#TOBEORNOTTOBEORTOBEORNOT";
        assert_eq!(decompress(&compress(data, 16)).unwrap(), data);
    }

    #[test]
    fn round_trips_across_code_width_changes() {
        // Pseudo-random bytes with some repetition push the table past 9..12 bits.
        let mut x = 0x1234_5678u32;
        let data: Vec<u8> = (0..200_000)
            .map(|i| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                if i % 7 < 3 { b'a' + (i % 5) as u8 } else { (x % 40) as u8 }
            })
            .collect();
        for bits in [9, 12, 16] {
            assert_eq!(decompress(&compress(&data, bits)).unwrap(), data, "max_bits={bits}");
        }
    }

    #[test]
    fn empty_payload_decodes_to_nothing() {
        assert_eq!(decompress(&[0x1f, 0x9d, 0x90]).unwrap(), Vec::<u8>::new());
    }
}
