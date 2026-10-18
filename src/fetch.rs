//! `ipview fetch`: download ticket files from the OXE over SFTP.

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, bail};
use clap::Args;

use ipview::remote::{self, Connection, HostKeyProblem, Selection, Target, TrustNewKey};

#[derive(Args)]
#[command(group = clap::ArgGroup::new("selection").multiple(false))]
pub struct FetchArgs {
    /// OXE host name or IP address.
    host: String,
    #[arg(long, default_value_t = remote::DEFAULT_PORT)]
    port: u16,
    #[arg(short, long, default_value = remote::DEFAULT_USER)]
    user: String,
    /// Remote directory with the IP*.DAT files.
    #[arg(short, long, default_value = remote::DEFAULT_DIR)]
    dir: String,
    /// Download the newest N files.
    #[arg(long, value_name = "N", group = "selection")]
    last_files: Option<u32>,
    /// Download files changed within the last N days (default: 1).
    #[arg(long, value_name = "N", group = "selection")]
    last_days: Option<u32>,
    /// Download files changed within the last N hours.
    #[arg(long, value_name = "N", group = "selection")]
    last_hours: Option<u32>,
    /// Local folder (default: Downloads/IPView/<host>).
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Only list the matching files, do not download.
    #[arg(long)]
    list: bool,
    /// Trust the server's host key on first connection without asking.
    /// Compare the printed fingerprint with the one shown on the OXE.
    #[arg(long)]
    accept_host_key: bool,
}

impl FetchArgs {
    fn selection(&self) -> Selection {
        match (self.last_files, self.last_days, self.last_hours) {
            (Some(n), _, _) => Selection::LastFiles(n),
            (_, _, Some(n)) => Selection::LastHours(n),
            (_, Some(n), _) => Selection::LastDays(n),
            _ => Selection::default(),
        }
    }
}

pub fn run(args: FetchArgs) -> Result<()> {
    let target = Target { host: args.host.clone(), port: args.port, user: args.user.clone(), dir: args.dir.clone() };
    let password = match std::env::var("IPVIEW_SSH_PASSWORD") {
        Ok(p) => p,
        Err(_) => rpassword::prompt_password(format!("Password for {}@{}: ", target.user, target.host))
            .context("reading password")?,
    };
    let known_hosts = remote::known_hosts_path();
    let trust = if args.accept_host_key { TrustNewKey::Accept } else { TrustNewKey::Ask };

    let conn = match Connection::open(&target, &password, &known_hosts, trust) {
        Ok(c) => c,
        Err(e) => match e.downcast_ref::<HostKeyProblem>() {
            Some(HostKeyProblem::Unknown { fingerprint }) => {
                if !confirm_new_key(&target, fingerprint)? {
                    bail!("host key not trusted; nothing downloaded");
                }
                Connection::open(&target, &password, &known_hosts, TrustNewKey::Accept)?
            }
            _ => return Err(e),
        },
    };

    let files = conn.list()?;
    let selected = remote::select(&files, args.selection(), remote::now());
    eprintln!("{} ticket files in {}, {} selected", files.len(), target.dir, selected.len());
    if args.list {
        for f in &selected {
            println!("{:<16} {:>9} bytes  {}", f.name, f.size, ipview::ticket::format_timestamp(f.mtime));
        }
        conn.disconnect();
        return Ok(());
    }

    let dest = args.output.unwrap_or_else(|| remote::default_download_dir(&target.host));
    let cancel = AtomicBool::new(false);
    let (mut fetched, mut skipped) = (0, 0);
    for (i, f) in selected.iter().enumerate() {
        eprint!("\r[{}/{}] {}          ", i + 1, selected.len(), f.name);
        let (_, new) = conn.download(f, &dest, &cancel, |_| {})?;
        if new { fetched += 1 } else { skipped += 1 }
    }
    eprintln!();
    conn.disconnect();
    eprintln!("{fetched} downloaded, {skipped} already present, in {}", dest.display());
    Ok(())
}

fn confirm_new_key(target: &Target, fingerprint: &str) -> Result<bool> {
    eprintln!("The host key of {}:{} is not known yet.", target.host, target.port);
    eprintln!("Fingerprint: {fingerprint}");
    if !io::stdin().is_terminal() {
        bail!(
            "cannot ask for confirmation without a terminal; rerun with --accept-host-key after checking the fingerprint"
        );
    }
    eprint!("Trust this host and continue? [y/N] ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "j" | "J" | "ja"))
}
