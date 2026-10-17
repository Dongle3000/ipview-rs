//! Catalog of OXE VoIP ticket fields.
//!
//! IDs, names, value encodings and groupings were recovered from IPView 2.0.1.
//! Fields 500..=503 are not stored in tickets; IPView derived them, and so do we
//! (see [`crate::ticket::Metrics`]).

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// u32 big-endian seconds since 1970-01-01.
    Timestamp,
    U8,
    /// u16 big-endian; a 1-byte value is zero-extended.
    U16,
    /// u32 big-endian; shorter values are zero-extended.
    U32,
    /// One byte, non-zero means true.
    Bool,
    /// u32 big-endian, rendered as a dotted quad.
    Ipv4,
    /// Phone number: bytes 1..=9 are digits, 10 is "0", others are taken literally.
    Digits,
    /// Two bytes: equipment class and sub-type.
    Equipment,
    Codec,
    VoiceMode,
    /// u16, rendered as "major.minor" from the high and low byte.
    Version,
    /// u32, rendered as 8 hex digits.
    Hex32,
    /// Distribution of big-endian u16 counters.
    Buckets16,
    /// Distribution of big-endian u32 counters.
    Buckets32,
    /// Only the presence of the field matters.
    Present,
    /// Computed from other fields.
    Derived,
}

/// Field groups, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Group {
    Ticket,
    Quality,
    Call,
    Connection,
    Voice,
    Rtp,
    Delay,
    Jitter,
    Bfi,
    Fax,
    Transparency,
    QoS,
    Dsp,
    Misc,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Field {
    pub id: u16,
    /// Short identifier used on the command line and as CSV/JSON key.
    pub key: &'static str,
    pub name: &'static str,
    pub kind: Kind,
    pub group: Group,
    pub unit: Option<&'static str>,
    /// Labels for distribution buckets, in storage order.
    pub buckets: &'static [&'static str],
}

const fn f(id: u16, key: &'static str, name: &'static str, kind: Kind, group: Group) -> Field {
    Field { id, key, name, kind, group, unit: None, buckets: &[] }
}

const fn unit(mut field: Field, unit: &'static str) -> Field {
    field.unit = Some(unit);
    field
}

const fn buckets(mut field: Field, labels: &'static [&'static str]) -> Field {
    field.buckets = labels;
    field
}

const IDX10: &[&str] = &["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"];

use Group as G;
use Kind as K;

pub static FIELDS: &[Field] = &[
    f(1, "end_time", "End of communication", K::Timestamp, G::Ticket),
    f(2, "node", "Node number", K::U16, G::Connection),
    f(3, "ticket_version", "Ticket (protocol) version", K::U8, G::Ticket),
    f(4, "crystal", "Crystal number", K::U8, G::Connection),
    f(5, "coupler", "Coupler number", K::U8, G::Connection),
    f(6, "equipment", "Equipment type", K::Equipment, G::Call),
    f(7, "timeslot", "Timeslot", K::U8, G::Call),
    f(8, "local_ip", "Local IP", K::Ipv4, G::Connection),
    f(9, "distant_ip", "Distant IP", K::Ipv4, G::Connection),
    f(10, "local_id", "Local ID", K::Digits, G::Call),
    f(11, "distant_id", "Distant ID", K::Digits, G::Call),
    unit(f(12, "duration", "Call duration", K::U16, G::Call), "s"),
    f(13, "local_ssrc", "Local SSRC", K::Hex32, G::Call),
    f(14, "distant_ssrc", "Distant SSRC", K::Hex32, G::Call),
    f(15, "codec", "Compression type", K::Codec, G::Voice),
    f(16, "vad", "VAD", K::Bool, G::Voice),
    f(17, "echo_canceller", "Echo canceller", K::Bool, G::Voice),
    f(18, "voice_mode", "Voice mode", K::VoiceMode, G::Voice),
    unit(f(19, "framing_requested", "Requested framing duration", K::U8, G::Voice), "ms"),
    unit(f(20, "framing_received", "Received framing duration", K::U8, G::Voice), "ms"),
    f(21, "framing_changes", "Framing changes", K::U8, G::Voice),
    f(22, "rtp_rx", "RTP packets received", K::U32, G::Rtp),
    f(23, "rtp_tx", "RTP packets sent", K::U32, G::Rtp),
    f(24, "rtp_lost", "RTP packets lost", K::U16, G::Rtp),
    f(25, "total_silence", "Total silence", K::U16, G::Voice),
    f(26, "sid_rx", "SID packets received", K::U16, G::Voice),
    unit(
        buckets(
            f(27, "rtd_distribution", "Round-trip delay distribution", K::Buckets16, G::Delay),
            &["0-40", "40-80", "80-150", "150-250", "250+"],
        ),
        "ms",
    ),
    unit(f(28, "max_delay", "Maximum round-trip delay", K::U16, G::Delay), "ms"),
    f(29, "dtmf", "DTMF tones", K::U8, G::Voice),
    buckets(f(30, "bfi_consecutive", "Consecutive BFI", K::Buckets16, G::Bfi), IDX10),
    buckets(
        f(31, "bfi_distribution", "BFI distribution", K::Buckets16, G::Bfi),
        &["0%", "0-1%", "1-2%", "2-3%", ">3%"],
    ),
    buckets(f(32, "jitter_depth", "Jitter buffer depth", K::Buckets32, G::Jitter), IDX10),
    f(33, "icmp", "ICMP errors", K::U16, G::Connection),
    f(34, "fax_lost", "FAX packets lost", K::U32, G::Fax),
    buckets(f(35, "fax_underrun_t4", "FAX underrun T4", K::Buckets16, G::Fax), &["1-6", "6-11", "11-16", "16+"]),
    f(36, "fax_underrun_v21", "FAX underrun V21", K::U16, G::Fax),
    buckets(f(37, "net_jitter_distribution", "Network jitter distribution", K::Buckets32, G::Jitter), IDX10),
    f(38, "sw_version", "Software version", K::Version, G::Call),
    f(39, "mcdu", "Terminal MCDU", K::Digits, G::Call),
    f(40, "network", "Network number", K::U16, G::Connection),
    unit(f(41, "dsp_framing", "DSP framing duration", K::U8, G::Voice), "ms"),
    f(42, "sid_tx", "SID packets sent", K::U16, G::Voice),
    f(43, "data_transparency", "Data transparency", K::Bool, G::Transparency),
    f(44, "modem_transparency", "Modem transparency", K::Bool, G::Transparency),
    unit(f(45, "min_delay", "Minimum round-trip delay", K::U16, G::Delay), "ms"),
    f(46, "qos_8021q", "802.1Q used", K::Bool, G::QoS),
    f(47, "qos_priority", "802.1Q priority", K::U8, G::QoS),
    f(48, "vlan", "VLAN ID", K::U16, G::QoS),
    f(49, "dscp", "DiffServ (DSCP)", K::U8, G::QoS),
    f(55, "encryption", "Ticket encryption", K::Bool, G::Misc),
    f(60, "local_jetlag", "Local jetlag", K::U16, G::Misc),
    buckets(
        f(61, "bfi_over_200ms", "BFI distribution over 200 ms", K::Buckets16, G::Bfi),
        &["<10%", "<20%", "<40%", "<60%", ">=60%"],
    ),
    buckets(
        f(62, "rtp_consecutive_lost", "Consecutive RTP packets lost", K::Buckets16, G::Rtp),
        &["1", "2", "3", "4", "5+"],
    ),
    f(80, "transit", "Transit", K::Bool, G::Misc),
    f(81, "dsp_restarts", "DSP restarts", K::U8, G::Misc),
    f(82, "eth_restarts", "Ethernet restarts", K::U8, G::Misc),
    f(83, "hdlc_problem", "HDLC problem", K::Bool, G::Misc),
    f(84, "comm_type", "Communication type", K::U8, G::Misc),
    f(85, "dsp_erl", "DSP 6713 estimated ERL", K::U32, G::Dsp),
    f(86, "dsp_delay", "DSP 6713 estimated delay", K::U32, G::Dsp),
    f(87, "dsp_noise", "DSP 6713 background noise", K::U32, G::Dsp),
    f(88, "hpec_debug", "DSP 6713 HPEC debug data", K::Present, G::Dsp),
    f(89, "transp_buffer_depth", "Transparency buffer depth", K::U16, G::Transparency),
    f(90, "transp_underruns", "Transparency underrun count", K::U32, G::Transparency),
    unit(f(500, "loss_pct", "RTP packet loss", K::Derived, G::Quality), "%"),
    unit(f(501, "rtd", "Network round-trip delay", K::Derived, G::Quality), "ms"),
    unit(f(502, "jitter_delay", "Jitter delay", K::Derived, G::Quality), "ms"),
    f(503, "mos", "MOS", K::Derived, G::Quality),
];

/// Columns IPView shows by default (from its ipview.ini).
pub const DEFAULT_COLUMNS: &[u16] = &[1, 8, 9, 10, 11, 12, 22, 23, 24, 500, 503];

pub fn by_id(id: u16) -> Option<&'static Field> {
    FIELDS.iter().find(|f| f.id == id)
}

/// Looks a field up by numeric ID, key, or case-insensitive name.
pub fn lookup(name: &str) -> Option<&'static Field> {
    let name = name.trim();
    if let Ok(id) = name.parse::<u16>() {
        return by_id(id);
    }
    FIELDS.iter().find(|f| f.key.eq_ignore_ascii_case(name) || f.name.eq_ignore_ascii_case(name))
}

pub fn equipment_name(class: u8, sub: u8) -> String {
    match (class, sub) {
        (1, 1) => "IP-Phone v2".into(),
        (1, 2) => "NOE-IP".into(),
        (1, n) => format!("IP-Phone {n}"),
        (2, 0) => "4980 Softphone (PCMM2)".into(),
        (2, 1) => "WebSoftphoneIP".into(),
        (2, n) => format!("Appli-PC {n}"),
        (3, 0) => "IntIP".into(),
        (3, 1) => "GD".into(),
        (3, 2) => "eVA".into(),
        (3, n) => format!("OXE {n}"),
        (4, n) => format!("Cpl-OmniPCX-Off {n}"),
        (c, n) => format!("unknown {c}/{n}"),
    }
}

pub fn codec_name(codec: u8) -> String {
    match codec {
        0 => "G711A".into(),
        1 => "G711U".into(),
        2 => "G723 6.3kb/s".into(),
        3 => "G729A".into(),
        4 => "G723 5.3kb/s".into(),
        // Unknown to IPView 2.0.1. Inferred from newer OXE tickets: 5 runs on NOE
        // phones with G.711-like 5 ms DSP framing, 8 with 20 ms frame-codec framing.
        5 => "G722".into(),
        8 => "Opus".into(),
        n => format!("codec {n}"),
    }
}

pub fn voice_mode_name(mode: u8) -> String {
    match mode {
        0x50 => "Idle".into(),
        0x51 => "Handset".into(),
        0x52 => "Group-listening".into(),
        0x53 => "On-hook-dial".into(),
        0x54 => "Hands-Free".into(),
        0x55 => "Headset Jackplug".into(),
        n => n.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_and_keys_are_unique() {
        let ids: HashSet<_> = FIELDS.iter().map(|f| f.id).collect();
        let keys: HashSet<_> = FIELDS.iter().map(|f| f.key).collect();
        assert_eq!(ids.len(), FIELDS.len());
        assert_eq!(keys.len(), FIELDS.len());
    }

    #[test]
    fn lookup_accepts_id_key_and_name() {
        assert_eq!(lookup("503").unwrap().key, "mos");
        assert_eq!(lookup("MOS").unwrap().id, 503);
        assert_eq!(lookup("local ip").unwrap().id, 8);
        assert!(lookup("nope").is_none());
    }

    #[test]
    fn default_columns_exist() {
        assert!(DEFAULT_COLUMNS.iter().all(|&id| by_id(id).is_some()));
    }
}
