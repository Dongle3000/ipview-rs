//! CSV and JSON export, shared by the CLI and the GUI.

use std::io::Write;

use anyhow::{Result, bail};

use crate::fields;
use crate::ticket::{Ticket, TicketRecord, Value};

/// Writes all catalog fields, one row per ticket, with field keys as header.
pub fn csv<'a>(sink: impl Write, tickets: impl IntoIterator<Item = &'a Ticket>, delimiter: char) -> Result<()> {
    if !delimiter.is_ascii() {
        bail!("CSV delimiter must be an ASCII character");
    }
    let mut w = ::csv::WriterBuilder::new().delimiter(delimiter as u8).from_writer(sink);
    w.write_record(fields::FIELDS.iter().map(|f| f.key))?;
    for t in tickets {
        w.write_record(fields::FIELDS.iter().map(|f| t.value(f.id).map(|v| v.to_string()).unwrap_or_default()))?;
    }
    w.flush()?;
    Ok(())
}

/// Writes a JSON array of objects keyed by field key.
pub fn json<'a>(mut sink: impl Write, tickets: impl IntoIterator<Item = &'a Ticket>) -> Result<()> {
    let records: Vec<TicketRecord> = tickets.into_iter().map(TicketRecord).collect();
    serde_json::to_writer_pretty(&mut sink, &records)?;
    writeln!(sink)?;
    Ok(())
}

/// Orders by value, numerically where possible; tickets lacking the field sort last.
pub fn cmp_values(a: Option<&Value>, b: Option<&Value>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => x.total_cmp(&y),
            _ => a.to_string().cmp(&b.to_string()),
        },
    }
}
