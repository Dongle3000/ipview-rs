//! OXE VoIP ticket records.
//!
//! A ticket file is a flat stream of TLV entries:
//!
//! ```text
//! [field id: u16 little-endian][length: u8][value: length bytes]
//! ```
//!
//! A new ticket starts at every entry with field ID 1 (end-of-communication
//! timestamp). Values themselves are big-endian.

use std::collections::BTreeMap;
use std::fmt;
use std::net::Ipv4Addr;

use chrono::DateTime;
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

use crate::emodel;
use crate::fields::{self, Field, Kind};

pub const START_FIELD: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    raw: Vec<u8>,
    fields: BTreeMap<u16, Vec<u8>>,
}

/// Splits a decompressed ticket stream into tickets. Trailing bytes too short
/// to form an entry are ignored, as in IPView.
pub fn split(stream: &[u8]) -> Vec<Ticket> {
    let mut tickets = Vec::new();
    let mut start = 0;
    let mut pos = 0;
    while pos + 3 <= stream.len() {
        let id = u16::from_le_bytes([stream[pos], stream[pos + 1]]);
        if id == START_FIELD && pos > start {
            tickets.push(Ticket::from_raw(&stream[start..pos]));
            start = pos;
        }
        pos = (pos + 3 + stream[pos + 2] as usize).min(stream.len());
    }
    if pos > start {
        tickets.push(Ticket::from_raw(&stream[start..pos]));
    }
    tickets
}

impl Ticket {
    /// Parses one ticket's TLV entries. Later duplicates of a field replace earlier ones.
    pub fn from_raw(raw: &[u8]) -> Ticket {
        let mut fields = BTreeMap::new();
        let mut pos = 0;
        while pos + 3 <= raw.len() {
            let id = u16::from_le_bytes([raw[pos], raw[pos + 1]]);
            let end = (pos + 3 + raw[pos + 2] as usize).min(raw.len());
            fields.insert(id, raw[pos + 3..end].to_vec());
            pos = end;
        }
        Ticket { raw: raw.to_vec(), fields }
    }

    /// The ticket's bytes exactly as read, for re-saving.
    pub fn raw(&self) -> &[u8] {
        &self.raw
    }

    pub fn raw_field(&self, id: u16) -> Option<&[u8]> {
        self.fields.get(&id).map(Vec::as_slice)
    }

    /// IDs of all fields present, including ones not in the catalog.
    pub fn field_ids(&self) -> impl Iterator<Item = u16> + '_ {
        self.fields.keys().copied()
    }

    fn uint(&self, id: u16, width: usize) -> Option<u32> {
        let v = self.raw_field(id)?;
        if v.is_empty() {
            return None;
        }
        // Short values are zero-extended on the left; long ones are truncated.
        let v = &v[..v.len().min(width)];
        Some(v.iter().fold(0u32, |acc, &b| acc << 8 | b as u32))
    }

    pub fn u8(&self, id: u16) -> Option<u8> {
        self.raw_field(id)?.first().copied()
    }

    pub fn u16(&self, id: u16) -> Option<u16> {
        self.uint(id, 2).map(|v| v as u16)
    }

    pub fn u32(&self, id: u16) -> Option<u32> {
        self.uint(id, 4)
    }

    pub fn buckets(&self, id: u16, width: usize) -> Vec<u32> {
        self.raw_field(id)
            .unwrap_or_default()
            .chunks_exact(width)
            .map(|c| c.iter().fold(0u32, |acc, &b| acc << 8 | b as u32))
            .collect()
    }

    pub fn end_time(&self) -> Option<i64> {
        self.u32(1).map(i64::from)
    }

    pub fn equipment(&self) -> Option<(u8, u8)> {
        match self.raw_field(6)? {
            [class, sub, ..] => Some((*class, *sub)),
            _ => None,
        }
    }

    pub fn is_noe(&self) -> bool {
        self.equipment() == Some((1, 2))
    }

    /// Decodes a field according to the catalog. Returns `None` if absent.
    pub fn value(&self, id: u16) -> Option<Value> {
        let field = fields::by_id(id);
        let kind = field.map_or(Kind::Present, |f| f.kind);
        if kind == Kind::Derived {
            let m = self.metrics();
            return match id {
                500 => m.loss_pct.map(Value::Float),
                501 => m.rtd.map(Value::Float),
                502 => m.jitter_delay.map(Value::Float),
                503 => m.mos.map(Value::Float),
                _ => None,
            };
        }
        let raw = self.raw_field(id)?;
        Some(match kind {
            Kind::Timestamp => Value::Timestamp(self.u32(id)? as i64),
            Kind::U8 => Value::Int(self.u8(id)? as u64),
            Kind::U16 => Value::Int(self.u16(id)? as u64),
            Kind::U32 => Value::Int(self.u32(id)? as u64),
            Kind::Bool => Value::Bool(self.u8(id)? != 0),
            Kind::Ipv4 => Value::Ip(Ipv4Addr::from(self.u32(id)?)),
            Kind::Digits => Value::Text(decode_digits(raw)),
            Kind::Equipment => {
                let (c, s) = self.equipment()?;
                Value::Text(fields::equipment_name(c, s))
            }
            Kind::Codec => Value::Text(fields::codec_name(self.u8(id)?)),
            Kind::VoiceMode => Value::Text(fields::voice_mode_name(self.u8(id)?)),
            Kind::Version => {
                let v = self.u16(id)?;
                Value::Text(format!("{}.{}", v >> 8, v & 0xff))
            }
            Kind::Hex32 => Value::Text(format!("{:08X}", self.u32(id)?)),
            Kind::Buckets16 => Value::Buckets(self.buckets(id, 2)),
            Kind::Buckets32 => Value::Buckets(self.buckets(id, 4)),
            Kind::Present => Value::Raw(raw.to_vec()),
            Kind::Derived => unreachable!(),
        })
    }

    pub fn metrics(&self) -> Metrics {
        Metrics::compute(self)
    }
}

fn decode_digits(raw: &[u8]) -> String {
    let s: String = raw
        .iter()
        .map(|&b| match b {
            1..=9 => (b'0' + b) as char,
            10 => '0',
            _ => b as char,
        })
        .collect();
    s.trim_matches(|c: char| c <= ' ').to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Value {
    Int(u64),
    Float(f64),
    Bool(bool),
    Text(String),
    Ip(Ipv4Addr),
    #[serde(serialize_with = "ser_timestamp")]
    Timestamp(i64),
    Buckets(Vec<u32>),
    #[serde(serialize_with = "ser_hex")]
    Raw(Vec<u8>),
}

fn ser_timestamp<S: Serializer>(secs: &i64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format_timestamp(*secs))
}

fn ser_hex<S: Serializer>(raw: &[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&hex(raw))
}

fn hex(raw: &[u8]) -> String {
    raw.iter().map(|b| format!("{b:02x}")).collect()
}

/// IPView converted the timestamp without any time-zone adjustment, so it is shown as UTC.
pub fn format_timestamp(secs: i64) -> String {
    DateTime::from_timestamp(secs, 0)
        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| secs.to_string())
}

impl Value {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(v) => Some(*v as f64),
            Value::Float(v) => Some(*v),
            Value::Bool(v) => Some(*v as u8 as f64),
            Value::Timestamp(v) => Some(*v as f64),
            Value::Buckets(v) => Some(v.iter().map(|&x| x as f64).sum()),
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(v) => write!(f, "{v}"),
            Value::Float(v) => write!(f, "{v:.2}"),
            Value::Bool(v) => f.write_str(if *v { "yes" } else { "no" }),
            Value::Text(v) => f.write_str(v),
            Value::Ip(v) => write!(f, "{v}"),
            Value::Timestamp(v) => f.write_str(&format_timestamp(*v)),
            Value::Buckets(v) => {
                let parts: Vec<String> = v.iter().map(u32::to_string).collect();
                f.write_str(&parts.join("/"))
            }
            Value::Raw(v) => f.write_str(&hex(v)),
        }
    }
}

/// Values IPView derived from the raw counters (its field IDs 500..=503).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct Metrics {
    /// RTP packet loss in percent of expected packets.
    pub loss_pct: Option<f64>,
    /// Mean network round-trip delay (ms), from the delay distribution.
    pub rtd: Option<f64>,
    /// Mean jitter buffer delay (ms).
    pub jitter_delay: Option<f64>,
    /// E-model MOS estimate.
    pub mos: Option<f64>,
}

// Per-codec tables, indexed by field 15 (G711A, G711U, G723 6.3, G729A, G723 5.3).
const FRAME_MS: [f64; 5] = [10.0, 10.0, 30.0, 10.0, 10.0];
const FRAME_MS_NOE: [f64; 5] = [5.0, 5.0, 30.0, 10.0, 5.0];
const IE: [f64; 5] = [0.0, 0.0, 15.0, 11.0, 0.0];
const BPL: [f64; 5] = [25.1, 25.1, 16.1, 19.0, 4.3];
const CODEC_DELAY_MS: [f64; 5] = [20.0, 20.0, 92.0, 16.0, 20.0];
/// Weights for buckets 1..=4 of field 61 (BFI over 200 ms) on NOE phones.
const BFI200_WEIGHTS: [f64; 4] = [30.0, 60.0, 100.0, 200.0];

impl Metrics {
    pub fn compute(t: &Ticket) -> Metrics {
        let rx = t.u32(22).unwrap_or(0) as f64;
        let lost = t.u16(24).unwrap_or(0) as f64;
        let loss_pct = (rx + lost > 0.0).then(|| 100.0 / (rx + lost) * lost);

        let rtd_buckets = t.buckets(27, 2);
        let rtd = (!rtd_buckets.is_empty()).then(|| round_trip(&rtd_buckets, t.u16(28).unwrap_or(0)));

        let framing = t.u8(20).unwrap_or(0) as f64;
        let jitter = t.buckets(32, 4);
        let jitter_delay = (!jitter.is_empty()).then(|| jitter_delay(&jitter, framing));

        let mos = t
            .u8(15)
            .filter(|&c| (c as usize) < IE.len())
            .map(|codec| mos(t, codec as usize, rtd.unwrap_or(0.0), framing, jitter_delay.unwrap_or(0.0)));

        Metrics { loss_pct, rtd, jitter_delay, mos }
    }
}

/// Weighted mean of the round-trip delay distribution, using bucket midpoints
/// and the maximum delay (capped at 500 ms) for the open-ended last bucket.
fn round_trip(buckets: &[u32], max_delay: u16) -> f64 {
    let mids = [20.0, 60.0, 115.0, 200.0, max_delay.min(500) as f64];
    let (sum, n) = buckets.iter().zip(mids).fold((0.0, 0.0), |(s, n), (&c, m)| (s + c as f64 * m, n + c as f64));
    if n > 0.0 { sum / n } else { 0.0 }
}

/// Weighted mean jitter buffer depth: bucket `i` stands for `(i + 1)` frames.
fn jitter_delay(buckets: &[u32], framing: f64) -> f64 {
    let (weighted, n) = buckets
        .iter()
        .take(10)
        .enumerate()
        .fold((0.0, 0.0), |(w, n), (i, &c)| (w + c as f64 * (i + 1) as f64 * framing, n + c as f64));
    if weighted != 0.0 { weighted / n } else { 0.0 }
}

fn mos(t: &Ticket, codec: usize, rtd: f64, framing: f64, jitter_delay: f64) -> f64 {
    let noe = t.is_noe();
    let lost = t.u16(24).unwrap_or(0) as f64;
    let mut p = emodel::Params {
        stmr: if noe { 22.0 } else { 15.0 },
        lstr: 14.5,
        rlr: if noe { 2.0 } else { 3.0 },
        slr: if noe { 8.0 } else { 7.0 },
        telr: if noe { 66.0 } else { 55.0 },
        ie: IE[codec],
        bpl: BPL[codec],
        ta: rtd / 2.0 + framing + CODEC_DELAY_MS[codec] + jitter_delay,
        ..Default::default()
    };

    // Estimate the number of lost frames.
    let bfi_cons = t.buckets(30, 2);
    let bfi200 = t.buckets(61, 2);
    let bucket = |v: &[u32], i: usize| v.get(i).copied().unwrap_or(0) as f64;
    let lost_frames = if noe {
        let frame = FRAME_MS_NOE[codec];
        // IPView overwrites the estimate on every iteration instead of summing,
        // so only the last bucket counts. Kept for identical results.
        let mut x = 0.0;
        for (k, w) in BFI200_WEIGHTS.iter().enumerate() {
            x = (w * bucket(&bfi200, k + 1) / frame).round_ties_even();
        }
        x.max(lost * framing / frame).round_ties_even()
    } else {
        let x: f64 = (0..9).map(|i| (i + 1) as f64 * bucket(&bfi_cons, i)).sum();
        x.max(lost * framing / FRAME_MS[codec]).round_ties_even()
    };

    let total_frames: f64 = t.buckets(32, 4).iter().take(10).map(|&c| c as f64).sum();
    if total_frames != 0.0 {
        p.ppl = lost_frames / total_frames * 100.0;
    }

    p.burst_r = if p.ppl == 0.0 {
        1.0
    } else {
        let events: f64 =
            if noe { (1..=4).map(|i| bucket(&bfi200, i)).sum() } else { (0..10).map(|i| bucket(&bfi_cons, i)).sum() };
        let loss = p.ppl / 100.0;
        let br = if events != 0.0 && loss != 1.0 { lost_frames / events * (1.0 - loss) } else { 0.0 };
        br.clamp(1.0, 10.0)
    };

    p.mos()
}

/// Serializes a ticket as a flat map of field key to decoded value.
pub struct TicketRecord<'a>(pub &'a Ticket);

impl Serialize for TicketRecord<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let t = self.0;
        let mut map = s.serialize_map(None)?;
        for field in fields::FIELDS {
            if let Some(v) = t.value(field.id) {
                map.serialize_entry(field.key, &v)?;
            }
        }
        for id in t.field_ids().filter(|&id| fields::by_id(id).is_none()) {
            map.serialize_entry(&format!("field_{id}"), &Value::Raw(t.raw_field(id).unwrap().to_vec()))?;
        }
        map.end()
    }
}

/// Human-readable value with unit, or an empty string if absent.
pub fn display(t: &Ticket, field: &Field) -> String {
    match t.value(field.id) {
        Some(v @ Value::Buckets(_)) => v.to_string(),
        Some(v) => match field.unit {
            Some(u) => format!("{v} {u}"),
            None => v.to_string(),
        },
        None => String::new(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn tlv(entries: &[(u16, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (id, v) in entries {
            out.extend(id.to_le_bytes());
            out.push(v.len() as u8);
            out.extend(*v);
        }
        out
    }

    #[test]
    fn splits_on_field_one() {
        let mut stream = tlv(&[(1, &[0, 0, 0, 1]), (12, &[0, 5])]);
        stream.extend(tlv(&[(1, &[0, 0, 0, 2]), (12, &[0, 6])]));
        let tickets = split(&stream);
        assert_eq!(tickets.len(), 2);
        assert_eq!(tickets[1].u16(12), Some(6));
        assert_eq!(tickets.iter().flat_map(|t| t.raw().to_vec()).collect::<Vec<_>>(), stream);
    }

    #[test]
    fn decodes_typed_values() {
        let t = Ticket::from_raw(&tlv(&[
            (1, &0x6a8e_b23bu32.to_be_bytes()),
            (6, &[1, 2]),
            (8, &[192, 168, 91, 13]),
            (10, &[8, 5, 1, 10, 0, 0]),
            (15, &[3]),
            (16, &[1]),
            (24, &[7]),
            (38, &[0x10, 0x02]),
            (60, &[2]),
        ]));
        assert_eq!(t.value(1).unwrap().to_string(), "2026-08-26 09:30:35");
        assert_eq!(t.value(6).unwrap().to_string(), "NOE-IP");
        assert_eq!(t.value(8).unwrap().to_string(), "192.168.91.13");
        assert_eq!(t.value(10).unwrap().to_string(), "8510");
        assert_eq!(t.value(15).unwrap().to_string(), "G729A");
        assert_eq!(t.value(16), Some(Value::Bool(true)));
        assert_eq!(t.value(24), Some(Value::Int(7)), "1-byte word is zero-extended");
        assert_eq!(t.value(38).unwrap().to_string(), "16.2");
        assert_eq!(t.value(60), Some(Value::Int(2)));
        assert_eq!(t.value(99), None);
    }

    #[test]
    fn derived_metrics() {
        let t = Ticket::from_raw(&tlv(&[
            (15, &[0]),
            (20, &[20]),
            (22, &[0, 0, 0, 90]),
            (24, &[0, 10]),
            (27, &[0, 1, 0, 1, 0, 0, 0, 0, 0, 0]),
            (
                32,
                &[
                    0, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0,
                ],
            ),
        ]));
        let m = t.metrics();
        assert_eq!(m.loss_pct, Some(10.0));
        assert_eq!(m.rtd, Some(40.0));
        assert_eq!(m.jitter_delay, Some(25.0)); // (3*1 + 1*2) * 20 / 4
        let mos = m.mos.unwrap();
        assert!((1.0..=4.5).contains(&mos));
    }

    #[test]
    fn clean_call_scores_well() {
        let t = Ticket::from_raw(&tlv(&[(15, &[0]), (20, &[20]), (22, &[0, 0, 1, 0]), (24, &[0, 0])]));
        assert!(t.metrics().mos.unwrap() > 4.0);
    }

    #[test]
    fn unknown_codec_has_no_mos() {
        let t = Ticket::from_raw(&tlv(&[(15, &[9])]));
        assert_eq!(t.metrics().mos, None);
    }
}
