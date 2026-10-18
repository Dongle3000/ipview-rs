#[cfg(feature = "sftp")]
mod fetch;

use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};

use ipview::fields::{self, Field, Group};
use ipview::filter::{self, Condition, Filter};
use ipview::stats::{MOS_BANDS, Stats, Summary};
use ipview::ticket::{self, Ticket};
use ipview::{Value, export, file};

#[derive(Parser)]
#[command(version, about = "Analyze Alcatel-Lucent OmniPCX Enterprise VoIP tickets (IP*.DAT / .ipv)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print a table of tickets.
    List {
        #[command(flatten)]
        input: Input,
        /// Comma-separated columns (field keys or IDs). Default: IPView's default columns.
        #[arg(short, long, value_delimiter = ',')]
        columns: Vec<String>,
    },
    /// Show all fields of each matching ticket.
    Show {
        #[command(flatten)]
        input: Input,
    },
    /// Export matching tickets to CSV or JSON.
    Export {
        #[command(flatten)]
        input: Input,
        #[arg(short, long, value_enum, default_value_t = ExportFormat::Csv)]
        format: ExportFormat,
        /// Output file (default: stdout).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// CSV field delimiter.
        #[arg(long, default_value_t = ',')]
        delimiter: char,
    },
    /// Save matching tickets as an .ipv file (readable by IPView 2.0.1).
    Save {
        #[command(flatten)]
        input: Input,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Summary statistics for matching tickets.
    Stats {
        #[command(flatten)]
        input: Input,
    },
    /// List known ticket fields.
    Fields,
    /// Download IP*.DAT files from the OXE over SFTP.
    #[cfg(feature = "sftp")]
    Fetch(fetch::FetchArgs),
}

#[derive(Args)]
struct Input {
    /// Ticket files or directories containing IP*.DAT / *.ipv files.
    #[arg(required = true)]
    paths: Vec<PathBuf>,
    /// Filter, e.g. "mos<3.5", "equipment=NOE-IP", "local_ip~10.1.". Repeatable.
    #[arg(short = 'w', long = "where", value_name = "FIELD OP VALUE")]
    conditions: Vec<Condition>,
    /// Combine --where conditions with OR instead of AND.
    #[arg(long)]
    any: bool,
    /// Only calls between these two IPs (either direction).
    #[arg(long, num_args = 2, value_names = ["IP_A", "IP_B"])]
    between: Vec<String>,
    /// Sort by this field (key or ID).
    #[arg(short, long)]
    sort: Option<String>,
    /// Reverse the sort order.
    #[arg(short, long)]
    reverse: bool,
    /// Show at most this many tickets.
    #[arg(short = 'n', long)]
    limit: Option<usize>,
}

#[derive(Clone, Copy, ValueEnum)]
enum ExportFormat {
    Csv,
    Json,
}

fn main() {
    if let Err(e) = run() {
        // A closed pipe (e.g. `| head`) is not an error worth reporting.
        if e.downcast_ref::<io::Error>().is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe) {
            return;
        }
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut out = io::BufWriter::new(io::stdout().lock());
    match cli.command {
        Command::List { input, columns } => {
            let columns = resolve_columns(&columns)?;
            print_table(&mut out, &load(&input)?, &columns)?;
        }
        Command::Show { input } => {
            for (i, t) in load(&input)?.iter().enumerate() {
                if i > 0 {
                    writeln!(out)?;
                }
                print_ticket(&mut out, t)?;
            }
        }
        Command::Export { input, format, output, delimiter } => {
            let tickets = load(&input)?;
            let sink: Box<dyn Write> = match &output {
                Some(p) => Box::new(std::fs::File::create(p).with_context(|| format!("creating {}", p.display()))?),
                None => Box::new(&mut out),
            };
            match format {
                ExportFormat::Csv => export::csv(sink, &tickets, delimiter)?,
                ExportFormat::Json => export::json(sink, &tickets)?,
            }
            if let Some(p) = output {
                eprintln!("exported {} tickets to {}", tickets.len(), p.display());
            }
        }
        Command::Save { input, output } => {
            let n = file::write_ipv(&output, &load(&input)?)?;
            eprintln!("saved {n} tickets to {}", output.display());
        }
        Command::Stats { input } => print_stats(&mut out, &load(&input)?)?,
        #[cfg(feature = "sftp")]
        Command::Fetch(args) => fetch::run(args)?,
        Command::Fields => {
            for f in fields::FIELDS {
                writeln!(out, "{:>4}  {:<24} {:<32} {:?}", f.id, f.key, f.name, f.group)?;
            }
        }
    }
    out.flush()?;
    Ok(())
}

fn load(input: &Input) -> Result<Vec<Ticket>> {
    let filter = Filter { conditions: input.conditions.clone(), any: input.any };
    let mut tickets = Vec::new();
    let mut failed = 0;
    for path in file::expand(&input.paths)? {
        match file::read(&path) {
            Ok(ts) => tickets.extend(ts),
            Err(e) => {
                eprintln!("warning: {e:#}");
                failed += 1;
            }
        }
    }
    if failed > 0 {
        eprintln!("warning: {failed} file(s) could not be read");
    }
    tickets.retain(|t| {
        filter.matches(t) && (input.between.is_empty() || filter::between(t, &input.between[0], &input.between[1]))
    });
    if let Some(key) = &input.sort {
        let field = lookup(key)?;
        tickets.sort_by(|a, b| export::cmp_values(a.value(field.id).as_ref(), b.value(field.id).as_ref()));
    }
    if input.reverse {
        tickets.reverse();
    }
    if let Some(n) = input.limit {
        tickets.truncate(n);
    }
    Ok(tickets)
}

fn lookup(key: &str) -> Result<&'static Field> {
    fields::lookup(key).with_context(|| format!("unknown field '{key}' (see `ipview fields`)"))
}

fn resolve_columns(columns: &[String]) -> Result<Vec<&'static Field>> {
    if columns.is_empty() {
        return Ok(fields::DEFAULT_COLUMNS.iter().filter_map(|&id| fields::by_id(id)).collect());
    }
    columns.iter().map(|c| lookup(c)).collect()
}

fn print_table(out: &mut impl Write, tickets: &[Ticket], columns: &[&Field]) -> Result<()> {
    let rows: Vec<Vec<String>> = tickets
        .iter()
        .map(|t| columns.iter().map(|f| t.value(f.id).map(|v| v.to_string()).unwrap_or_default()).collect())
        .collect();
    let headers: Vec<String> = columns
        .iter()
        .map(|f| match f.unit {
            Some(u) => format!("{} [{u}]", f.key),
            None => f.key.to_string(),
        })
        .collect();
    let widths: Vec<usize> = (0..columns.len())
        .map(|i| rows.iter().map(|r| r[i].chars().count()).chain([headers[i].len()]).max().unwrap_or(0))
        .collect();
    let line = |cells: &[String]| {
        let padded: Vec<String> = cells.iter().zip(&widths).map(|(c, w)| format!("{c:<w$}")).collect();
        padded.join("  ").trim_end().to_string()
    };
    writeln!(out, "{}", line(&headers))?;
    for r in &rows {
        writeln!(out, "{}", line(r))?;
    }
    writeln!(out, "\n{} tickets", tickets.len())?;
    Ok(())
}

fn print_ticket(out: &mut impl Write, t: &Ticket) -> Result<()> {
    let title = t.value(1).map(|v| v.to_string()).unwrap_or_else(|| "(no timestamp)".into());
    writeln!(out, "=== {title} UTC")?;
    let mut ordered: Vec<&Field> = fields::FIELDS.iter().filter(|f| f.id != 1).collect();
    ordered.sort_by_key(|f| f.group);
    let mut group: Option<Group> = None;
    for f in ordered {
        let Some(value) = t.value(f.id) else { continue };
        if group != Some(f.group) {
            writeln!(out, "  [{:?}]", f.group)?;
            group = Some(f.group);
        }
        match &value {
            Value::Buckets(counts) => {
                let unit = f.unit.map(|u| format!(" ({u})")).unwrap_or_default();
                let cells: Vec<String> = counts
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("{}: {c}", f.buckets.get(i).copied().unwrap_or("?")))
                    .collect();
                writeln!(out, "    {:<32} {}{unit}", f.name, cells.join("  "))?;
            }
            _ => writeln!(out, "    {:<32} {}", f.name, ticket::display(t, f))?,
        }
    }
    let unknown: Vec<String> = t
        .field_ids()
        .filter(|&id| fields::by_id(id).is_none())
        .map(|id| format!("{id}={}", Value::Raw(t.raw_field(id).unwrap_or_default().to_vec())))
        .collect();
    if !unknown.is_empty() {
        writeln!(out, "  [Unknown]\n    {}", unknown.join("  "))?;
    }
    Ok(())
}

fn print_summary(out: &mut impl Write, name: &str, s: Option<Summary>) -> Result<()> {
    if let Some(s) = s {
        writeln!(
            out,
            "{name:<14} mean {:>7.2}  min {:>7.2}  p5 {:>7.2}  median {:>7.2}  p95 {:>7.2}  max {:>7.2}",
            s.mean, s.min, s.p5, s.median, s.p95, s.max
        )?;
    }
    Ok(())
}

fn print_stats(out: &mut impl Write, tickets: &[Ticket]) -> Result<()> {
    let s = Stats::compute(tickets);
    writeln!(out, "tickets: {}", s.tickets)?;
    if s.tickets == 0 {
        return Ok(());
    }
    if let Some((min, max)) = s.period {
        writeln!(out, "period:  {} .. {} UTC", ticket::format_timestamp(min), ticket::format_timestamp(max))?;
    }
    writeln!(out)?;
    print_summary(out, "MOS", s.mos)?;
    print_summary(out, "loss %", s.loss_pct)?;
    print_summary(out, "round-trip ms", s.rtd)?;
    print_summary(out, "jitter ms", s.jitter_delay)?;

    if let Some(mos) = s.mos {
        writeln!(out, "\nMOS bands:")?;
        for ((_, _, label), n) in MOS_BANDS.iter().zip(s.mos_bands) {
            writeln!(out, "  {label:<10} {n:>7}  ({:>5.1}%)", 100.0 * n as f64 / mos.count as f64)?;
        }
    }
    for (title, counts) in [("codecs", &s.codecs), ("equipment", &s.equipment)] {
        writeln!(out, "\n{title}:")?;
        for (k, n) in counts {
            writeln!(out, "  {k:<24} {n:>7}")?;
        }
    }
    Ok(())
}
