//! User-interface translations (English source strings, German translations).
//!
//! `t("English text")` returns the text in the current language; untranslated
//! strings fall back to English. `tf` fills `{name}` placeholders.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

use ipview::fields::{Field, Group};
use ipview::ticket::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LanguagePref {
    /// Follow the operating system's language.
    #[default]
    System,
    English,
    German,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn current() -> Lang {
    if CURRENT.load(Ordering::Relaxed) == Lang::De as u8 { Lang::De } else { Lang::En }
}

/// Applies a preference, resolving `System` from the OS locale.
pub fn apply(pref: LanguagePref) {
    set(match pref {
        LanguagePref::English => Lang::En,
        LanguagePref::German => Lang::De,
        LanguagePref::System => system_language(),
    });
}

/// German if the system locale is German (de, de-DE, de-AT, de-CH, …), else English.
pub fn system_language() -> Lang {
    language_for_locale(sys_locale::get_locale().as_deref().unwrap_or("en"))
}

fn language_for_locale(locale: &str) -> Lang {
    let primary = locale.split(['-', '_', '.']).next().unwrap_or("").to_ascii_lowercase();
    if primary == "de" { Lang::De } else { Lang::En }
}

/// Translates a UI string.
pub fn t(en: &'static str) -> &'static str {
    match current() {
        Lang::En => en,
        Lang::De => german().get(en).copied().unwrap_or(en),
    }
}

/// Translates and fills `{key}` placeholders.
pub fn tf(en: &'static str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut s = t(en).to_string();
    for (key, value) in args {
        s = s.replace(&format!("{{{key}}}"), &value.to_string());
    }
    s
}

fn german() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| DE.iter().copied().collect())
}

/// Field name in the current language.
pub fn field_name(f: &Field) -> &'static str {
    match current() {
        Lang::En => f.name,
        Lang::De => FIELDS_DE.iter().find(|(id, _)| *id == f.id).map_or(f.name, |(_, n)| n),
    }
}

pub fn group_title(g: Group) -> &'static str {
    t(match g {
        Group::Ticket => "Ticket",
        Group::Quality => "Quality",
        Group::Call => "Call",
        Group::Connection => "Connection",
        Group::Voice => "Voice",
        Group::Rtp => "RTP",
        Group::Delay => "Delay",
        Group::Jitter => "Jitter",
        Group::Bfi => "Bad frames (BFI)",
        Group::Fax => "FAX",
        Group::Transparency => "Transparency",
        Group::QoS => "QoS",
        Group::Dsp => "DSP",
        Group::Misc => "Miscellaneous",
    })
}

/// Display text for a value; booleans are translated.
pub fn value_text(v: &Value) -> String {
    match v {
        Value::Bool(b) => t(if *b { "yes" } else { "no" }).to_string(),
        v => v.to_string(),
    }
}

const FIELDS_DE: &[(u16, &str)] = &[
    (1, "Ende der Verbindung"),
    (2, "Knotennummer"),
    (3, "Ticketversion (Protokoll)"),
    (4, "Crystal-Nummer"),
    (5, "Koppler-Nummer"),
    (6, "Gerätetyp"),
    (7, "Zeitschlitz"),
    (8, "Lokale IP"),
    (9, "Entfernte IP"),
    (10, "Lokale ID"),
    (11, "Entfernte ID"),
    (12, "Gesprächsdauer"),
    (13, "Lokale SSRC"),
    (14, "Entfernte SSRC"),
    (15, "Kompressionsart"),
    (16, "VAD"),
    (17, "Echounterdrückung"),
    (18, "Sprachmodus"),
    (19, "Angeforderte Rahmendauer"),
    (20, "Empfangene Rahmendauer"),
    (21, "Änderungen der Rahmendauer"),
    (22, "RTP-Pakete empfangen"),
    (23, "RTP-Pakete gesendet"),
    (24, "RTP-Pakete verloren"),
    (25, "Stille gesamt"),
    (26, "SID-Pakete empfangen"),
    (27, "Verteilung der Umlaufverzögerung"),
    (28, "Maximale Umlaufverzögerung"),
    (29, "DTMF-Töne"),
    (30, "Aufeinanderfolgende BFI"),
    (31, "BFI-Verteilung"),
    (32, "Jitterpuffer-Tiefe"),
    (33, "ICMP-Fehler"),
    (34, "FAX-Pakete verloren"),
    (35, "FAX-Unterlauf T4"),
    (36, "FAX-Unterlauf V21"),
    (37, "Verteilung des Netzwerk-Jitters"),
    (38, "Softwareversion"),
    (39, "Endgeräte-MCDU"),
    (40, "Netznummer"),
    (41, "DSP-Rahmendauer"),
    (42, "SID-Pakete gesendet"),
    (43, "Datentransparenz"),
    (44, "Modemtransparenz"),
    (45, "Minimale Umlaufverzögerung"),
    (46, "802.1Q verwendet"),
    (47, "802.1Q-Priorität"),
    (48, "VLAN-ID"),
    (49, "DiffServ (DSCP)"),
    (55, "Ticket-Verschlüsselung"),
    (60, "Lokale Zeitverschiebung"),
    (61, "BFI-Verteilung über 200 ms"),
    (62, "Aufeinanderfolgend verlorene RTP-Pakete"),
    (80, "Transit"),
    (81, "DSP-Neustarts"),
    (82, "Ethernet-Neustarts"),
    (83, "HDLC-Problem"),
    (84, "Kommunikationstyp"),
    (85, "DSP 6713 geschätzter ERL"),
    (86, "DSP 6713 geschätzte Verzögerung"),
    (87, "DSP 6713 Hintergrundgeräusch"),
    (88, "DSP 6713 HPEC-Debugdaten"),
    (89, "Transparenz-Puffertiefe"),
    (90, "Transparenz-Unterläufe"),
    (500, "RTP-Paketverlust"),
    (501, "Netzwerk-Umlaufverzögerung"),
    (502, "Jitter-Verzögerung"),
    (503, "MOS"),
    (504, "R-Faktor (Breitband-Skala)"),
    (505, "Jitter-Zähler unplausibel"),
];

/// English → German. Keep the English side identical to the `t(…)` call sites.
const DE: &[(&str, &str)] = &[
    // Menus
    ("File", "Datei"),
    ("Open files…", "Dateien öffnen…"),
    ("Open folder…", "Ordner öffnen…"),
    ("Add files…", "Dateien hinzufügen…"),
    ("Add folder…", "Ordner hinzufügen…"),
    ("Download from OXE (SFTP)…", "Von OXE herunterladen (SFTP)…"),
    ("Save visible tickets as .ipv…", "Sichtbare Tickets als .ipv speichern…"),
    ("Export visible tickets to CSV…", "Sichtbare Tickets als CSV exportieren…"),
    ("Export visible tickets to JSON…", "Sichtbare Tickets als JSON exportieren…"),
    ("CSV delimiter", "CSV-Trennzeichen"),
    ("Semicolon (Excel, European)", "Semikolon (Excel, europäisch)"),
    ("Comma", "Komma"),
    ("Tab", "Tabulator"),
    ("Clear database", "Datenbank leeren"),
    ("Quit", "Beenden"),
    ("View", "Ansicht"),
    ("Columns…", "Spalten…"),
    ("Statistics", "Statistik"),
    ("Ticket details", "Ticketdetails"),
    ("Load errors", "Ladefehler"),
    ("Theme", "Farbschema"),
    ("Follow system", "Wie System"),
    ("Light", "Hell"),
    ("Dark", "Dunkel"),
    ("Language", "Sprache"),
    ("Help", "Hilfe"),
    ("About", "Über"),
    // Status bar and messages
    (
        "Open ticket files (IP*.DAT, *.ipv) with File > Open, F3, or drag and drop.",
        "Ticketdateien (IP*.DAT, *.ipv) über Datei > Öffnen, F3 oder per Drag & Drop öffnen.",
    ),
    ("Loaded {files} files.", "{files} Dateien geladen."),
    ("Loaded {files} files, {errors} could not be read.", "{files} Dateien geladen, {errors} nicht lesbar."),
    ("Cancelled. Loaded {files} files.", "Abgebrochen. {files} Dateien geladen."),
    ("Loading {done} / {total} files", "Lade {done} / {total} Dateien"),
    ("Cancel", "Abbrechen"),
    ("Tickets in database: {total}   Visible: {visible}", "Tickets in der Datenbank: {total}   Sichtbar: {visible}"),
    ("Saved {n} tickets to {path}.", "{n} Tickets in {path} gespeichert."),
    ("Saving failed: {error}", "Speichern fehlgeschlagen: {error}"),
    ("Exported {n} tickets to {path}.", "{n} Tickets nach {path} exportiert."),
    ("Export failed: {error}", "Export fehlgeschlagen: {error}"),
    ("Database cleared.", "Datenbank geleert."),
    ("Ticket files", "Ticketdateien"),
    ("All files", "Alle Dateien"),
    ("IPView file", "IPView-Datei"),
    // Table
    (
        "No tickets loaded.\nFile > Open (F3) or drop IP*.DAT files here.",
        "Keine Tickets geladen.\nDatei > Öffnen (F3) oder IP*.DAT-Dateien hierher ziehen.",
    ),
    (
        "Right-click a column header to sort, filter, move or hide it.",
        "Rechtsklick auf einen Spaltenkopf: sortieren, filtern, verschieben oder ausblenden.",
    ),
    (
        "{name} (field {id}). Click to sort, right-click for options.",
        "{name} (Feld {id}). Klicken zum Sortieren, Rechtsklick für Optionen.",
    ),
    ("Filter {op} {value}", "Filter {op} {value}"),
    ("Copy value", "Wert kopieren"),
    ("End time", "Ende"),
    ("Dur.", "Dauer"),
    ("RTP rx", "RTP empf."),
    ("RTP tx", "RTP ges."),
    ("RTP lost", "RTP verl."),
    ("Loss", "Verlust"),
    ("Round-trip", "Umlauf"),
    ("Jitter", "Jitter"),
    ("R (WB)", "R (WB)"),
    ("Jitter corrupt", "Jitter unplaus."),
    // Header menu and column picker
    ("Sort ascending", "Aufsteigend sortieren"),
    ("Sort descending", "Absteigend sortieren"),
    ("Clear sorting", "Sortierung aufheben"),
    ("Filter", "Filter"),
    ("Add filter", "Filter hinzufügen"),
    ("Remove filters on this column ({n})", "Filter dieser Spalte entfernen ({n})"),
    ("Move left", "Nach links"),
    ("Move right", "Nach rechts"),
    ("Hide column", "Spalte ausblenden"),
    ("Columns", "Spalten"),
    ("Defaults", "Standard"),
    ("Quality set", "Qualität"),
    ("End time, IPs, equipment, codec, loss, delays, MOS", "Ende, IPs, Gerät, Codec, Verlust, Verzögerungen, MOS"),
    ("field {id} · {key}", "Feld {id} · {key}"),
    ("value", "Wert"),
    ("contains", "enthält"),
    ("not contains", "enthält nicht"),
    ("Shown columns, in order:", "Angezeigte Spalten, in Reihenfolge:"),
    ("Available columns:", "Verfügbare Spalten:"),
    ("Up", "Hoch"),
    ("Down", "Runter"),
    // Filters
    ("Filters", "Filter"),
    ("Quality", "Qualität"),
    ("MOS ≤", "MOS ≤"),
    ("Loss % ≥", "Verlust % ≥"),
    ("e.g. 3.6", "z. B. 3.6"),
    ("e.g. 1", "z. B. 1"),
    ("Only implausible jitter counters", "Nur unplausible Jitter-Zähler"),
    ("Call", "Gespräch"),
    ("Equipment", "Gerät"),
    ("Codec", "Codec"),
    ("Number", "Rufnummer"),
    ("local or distant", "lokal oder entfernt"),
    ("Connection", "Verbindung"),
    ("IP", "IP"),
    ("and IP", "und IP"),
    ("calls between both", "Gespräche zwischen beiden"),
    ("Period (UTC)", "Zeitraum (UTC)"),
    ("From", "Von"),
    ("To", "Bis"),
    ("Field conditions", "Feldbedingungen"),
    ("Remove", "Entfernen"),
    ("+ Add condition", "+ Bedingung hinzufügen"),
    ("AND", "UND"),
    ("OR", "ODER"),
    ("Clear all filters (F6)", "Alle Filter löschen (F6)"),
    ("any", "alle"),
    // Detail view
    ("(no timestamp)", "(kein Zeitstempel)"),
    ("no MOS", "kein MOS"),
    ("loss {loss} %", "Verlust {loss} %"),
    ("! jitter counters implausible", "! Jitter-Zähler unplausibel"),
    (
        "Field 32 holds filler instead of counts. Jitter delay is not shown and MOS uses the RTP packet loss.",
        "Feld 32 enthält Füllwerte statt Zählern. Die Jitter-Verzögerung wird nicht angezeigt, der MOS verwendet den RTP-Paketverlust.",
    ),
    ("Select a ticket to see all of its fields.", "Ein Ticket auswählen, um alle Felder zu sehen."),
    ("Unknown fields", "Unbekannte Felder"),
    ("all zero", "alle null"),
    ("yes", "ja"),
    ("no", "nein"),
    ("Ticket", "Ticket"),
    ("Voice", "Sprachübertragung"),
    ("RTP", "RTP"),
    ("Delay", "Verzögerung"),
    ("Bad frames (BFI)", "Fehlerhafte Rahmen (BFI)"),
    ("FAX", "FAX"),
    ("Transparency", "Transparenz"),
    ("QoS", "QoS"),
    ("DSP", "DSP"),
    ("Miscellaneous", "Sonstiges"),
    // Statistics
    ("Statistics (visible tickets)", "Statistik (sichtbare Tickets)"),
    ("Tickets: {n}", "Tickets: {n}"),
    ("Period: {from} – {to} UTC", "Zeitraum: {from} – {to} UTC"),
    ("mean", "Mittel"),
    ("min", "Min"),
    ("p5", "P5"),
    ("median", "Median"),
    ("p95", "P95"),
    ("max", "Max"),
    ("MOS", "MOS"),
    ("Loss %", "Verlust %"),
    ("Round-trip ms", "Umlauf ms"),
    ("Jitter ms", "Jitter ms"),
    ("MOS bands", "MOS-Bereiche"),
    ("Codecs", "Codecs"),
    // About
    (
        "Analyzer for Alcatel-Lucent OmniPCX Enterprise VoIP tickets (IP*.DAT, .ipv).",
        "Analyse von Alcatel-Lucent OmniPCX Enterprise VoIP-Tickets (IP*.DAT, .ipv).",
    ),
    (
        "A rewrite of IPView 2.0.1 by Sven Fabricius (2007).",
        "Eine Neuentwicklung von IPView 2.0.1 von Sven Fabricius (2007).",
    ),
    (
        "MOS: ITU-T G.107 E-model (narrowband), G.107.1 (wideband, G.722).",
        "MOS: ITU-T G.107 E-Modell (Schmalband), G.107.1 (Breitband, G.722).",
    ),
    ("Times are shown in UTC.", "Zeiten werden in UTC angezeigt."),
    // Download dialog
    ("Download from OXE (SFTP)", "Von OXE herunterladen (SFTP)"),
    ("Host", "Host"),
    ("Port", "Port"),
    ("User", "Benutzer"),
    ("Password", "Passwort"),
    ("Remote folder", "Ordner auf der OXE"),
    ("Recent", "Zuletzt"),
    ("Download", "Herunterladen"),
    ("Last", "Die letzten"),
    ("files", "Dateien"),
    ("days", "Tage"),
    ("hours", "Stunden"),
    ("Local folder", "Lokaler Ordner"),
    ("Browse…", "Durchsuchen…"),
    ("Add to loaded tickets", "Zu geladenen Tickets hinzufügen"),
    ("Replace loaded tickets", "Geladene Tickets ersetzen"),
    ("Download and open", "Herunterladen und öffnen"),
    ("Close", "Schließen"),
    ("Connecting to {host}…", "Verbinde mit {host}…"),
    ("Listing files…", "Lese Dateiliste…"),
    ("Downloading {i} / {n}: {name}", "Lade {i} / {n}: {name}"),
    ("Enter the host name or IP address of the OXE.", "Hostname oder IP-Adresse der OXE eingeben."),
    ("Enter the password.", "Passwort eingeben."),
    ("The host key of this server is not known yet.", "Der Host-Schlüssel dieses Servers ist noch nicht bekannt."),
    (
        "Only continue if the fingerprint matches the one of your OXE.",
        "Nur fortfahren, wenn der Fingerabdruck mit dem Ihrer OXE übereinstimmt.",
    ),
    ("Trust and continue", "Vertrauen und fortfahren"),
    (
        "The host key of this server has CHANGED. Someone may be intercepting the connection. If the OXE was reinstalled, remove its entry from {path}.",
        "Der Host-Schlüssel dieses Servers hat sich GEÄNDERT. Möglicherweise wird die Verbindung abgehört. Wurde die OXE neu installiert, ihren Eintrag aus {path} entfernen.",
    ),
    ("Login failed: wrong user name or password?", "Anmeldung fehlgeschlagen: Benutzername oder Passwort falsch?"),
    ("Download failed: {error}", "Herunterladen fehlgeschlagen: {error}"),
    (
        "No files match the selection ({total} ticket files on the OXE).",
        "Keine Dateien entsprechen der Auswahl ({total} Ticketdateien auf der OXE).",
    ),
    (
        "{fetched} files downloaded, {skipped} already present, in {path}.",
        "{fetched} Dateien heruntergeladen, {skipped} bereits vorhanden, in {path}.",
    ),
    ("Default: {path}", "Standard: {path}"),
    ("The password is not saved.", "Das Passwort wird nicht gespeichert."),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_detection() {
        assert_eq!(language_for_locale("de-DE"), Lang::De);
        assert_eq!(language_for_locale("de_AT.UTF-8"), Lang::De);
        assert_eq!(language_for_locale("DE"), Lang::De);
        assert_eq!(language_for_locale("en-US"), Lang::En);
        assert_eq!(language_for_locale("fr-FR"), Lang::En);
        assert_eq!(language_for_locale(""), Lang::En);
    }

    #[test]
    fn placeholders_survive_translation() {
        for (en, de) in DE {
            let holes = |s: &str| {
                let mut v: Vec<String> =
                    s.split('{').skip(1).filter_map(|p| p.split_once('}').map(|(k, _)| k.to_string())).collect();
                v.sort();
                v
            };
            assert_eq!(holes(en), holes(de), "placeholders differ: {en:?} / {de:?}");
        }
    }

    #[test]
    fn no_duplicate_keys() {
        let mut seen = std::collections::HashSet::new();
        for (en, _) in DE {
            assert!(seen.insert(*en), "duplicate translation key {en:?}");
        }
    }

    #[test]
    fn every_field_has_a_german_name() {
        for f in ipview::fields::FIELDS {
            assert!(FIELDS_DE.iter().any(|(id, _)| *id == f.id), "no German name for field {}", f.id);
        }
    }

    /// Every literal passed to t()/tf() in the GUI sources must have a German entry.
    #[test]
    fn all_ui_strings_are_translated() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/ipview-gui");
        let mut missing = Vec::new();
        let mut found = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().is_some_and(|n| n == "i18n.rs") {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            for marker in ["t(\"", "tf(\""] {
                for (pos, _) in src.match_indices(marker) {
                    // Skip e.g. `hint_text("` or `selected_text("`: only standalone t( / tf(.
                    if src[..pos].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    let rest = &src[pos + marker.len()..];
                    let Some(end) = find_string_end(rest) else { continue };
                    let literal = unescape(&rest[..end]);
                    found += 1;
                    if !german().contains_key(literal.as_str()) {
                        missing.push(format!("{}: {literal:?}", path.file_name().unwrap().to_string_lossy()));
                    }
                }
            }
        }
        assert!(found > 150, "only {found} t()/tf() calls found; is the scanner broken?");
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "untranslated strings:\n{}", missing.join("\n"));
    }

    fn find_string_end(s: &str) -> Option<usize> {
        let mut escaped = false;
        for (i, c) in s.char_indices() {
            match c {
                '\\' if !escaped => escaped = true,
                '"' if !escaped => return Some(i),
                _ => escaped = false,
            }
        }
        None
    }

    fn unescape(s: &str) -> String {
        s.replace("\\n", "\n").replace("\\\"", "\"").replace("\\\\", "\\")
    }
}
