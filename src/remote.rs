//! Downloading ticket files from the OXE over SFTP.
//!
//! The OXE keeps VoIP tickets as `IP*.DAT` files, by default in
//! `/usr4/account`, readable by the `adfexc` account.

use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use ssh2::{CheckResult, HashType, KnownHostFileKind, Session};

pub const DEFAULT_USER: &str = "adfexc";
pub const DEFAULT_DIR: &str = "/usr4/account";
pub const DEFAULT_PORT: u16 = 22;

const TIMEOUT: Duration = Duration::from_secs(20);

/// Where to connect to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub dir: String,
}

impl Default for Target {
    fn default() -> Self {
        Target { host: String::new(), port: DEFAULT_PORT, user: DEFAULT_USER.into(), dir: DEFAULT_DIR.into() }
    }
}

/// Which of the remote ticket files to fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Selection {
    /// The newest `n` files.
    LastFiles(u32),
    /// Files modified within the last `n` days.
    LastDays(u32),
    /// Files modified within the last `n` hours.
    LastHours(u32),
}

impl Default for Selection {
    fn default() -> Self {
        Selection::LastDays(1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFile {
    pub name: String,
    pub size: u64,
    /// Modification time, Unix seconds.
    pub mtime: i64,
}

/// Ticket files as written by the OXE: `IP*.DAT`, any case.
pub fn is_remote_ticket_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("ip") && lower.ends_with(".dat")
}

/// Applies a selection; `now` is Unix seconds. Returns the files oldest first.
pub fn select(files: &[RemoteFile], selection: Selection, now: i64) -> Vec<RemoteFile> {
    let mut sorted: Vec<RemoteFile> = files.to_vec();
    sorted.sort_by(|a, b| a.mtime.cmp(&b.mtime).then_with(|| a.name.cmp(&b.name)));
    match selection {
        Selection::LastFiles(n) => {
            let skip = sorted.len().saturating_sub(n as usize);
            sorted.split_off(skip)
        }
        Selection::LastDays(n) => sorted.into_iter().filter(|f| f.mtime >= now - n as i64 * 86_400).collect(),
        Selection::LastHours(n) => sorted.into_iter().filter(|f| f.mtime >= now - n as i64 * 3_600).collect(),
    }
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The server's host key could not be confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyProblem {
    /// First connection: the key is not in known_hosts yet.
    Unknown { fingerprint: String },
    /// The key differs from the one stored earlier. Possibly a man-in-the-middle.
    Changed { fingerprint: String },
}

impl fmt::Display for HostKeyProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostKeyProblem::Unknown { fingerprint } => write!(f, "unknown host key {fingerprint}"),
            HostKeyProblem::Changed { fingerprint } => write!(
                f,
                "HOST KEY CHANGED (now {fingerprint}). This can mean someone is intercepting the \
                 connection. If the server was reinstalled, remove its line from the known_hosts file."
            ),
        }
    }
}

impl std::error::Error for HostKeyProblem {}

/// How to treat a host key that is not yet known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustNewKey {
    /// Fail with [`HostKeyProblem::Unknown`] so the user can confirm the fingerprint.
    Ask,
    /// Accept and remember it (after the user confirmed the fingerprint).
    Accept,
}

pub struct Connection {
    session: Session,
    sftp: ssh2::Sftp,
    target: Target,
}

impl Connection {
    /// Connects, verifies the host key against `known_hosts` and logs in with a password.
    pub fn open(target: &Target, password: &str, known_hosts: &Path, trust: TrustNewKey) -> Result<Connection> {
        let addr = (target.host.as_str(), target.port)
            .to_socket_addrs()
            .with_context(|| format!("resolving {}", target.host))?
            .next()
            .ok_or_else(|| anyhow!("no address for {}", target.host))?;
        let tcp = TcpStream::connect_timeout(&addr, TIMEOUT)
            .with_context(|| format!("connecting to {}:{}", target.host, target.port))?;

        let mut session = Session::new()?;
        session.set_timeout(TIMEOUT.as_millis() as u32);
        session.set_tcp_stream(tcp);
        session.handshake().context("SSH handshake failed")?;
        verify_host_key(&session, target, known_hosts, trust)?;

        login(&session, &target.user, password)?;
        let sftp = session.sftp().context("starting SFTP")?;
        Ok(Connection { session, sftp, target: target.clone() })
    }

    /// Ticket files in the target directory.
    pub fn list(&self) -> Result<Vec<RemoteFile>> {
        let entries =
            self.sftp.readdir(Path::new(&self.target.dir)).with_context(|| format!("listing {}", self.target.dir))?;
        Ok(entries
            .into_iter()
            .filter(|(_, stat)| stat.is_file())
            .filter_map(|(path, stat)| {
                let name = path.file_name()?.to_string_lossy().into_owned();
                is_remote_ticket_name(&name).then(|| RemoteFile {
                    name,
                    size: stat.size.unwrap_or(0),
                    mtime: stat.mtime.unwrap_or(0) as i64,
                })
            })
            .collect())
    }

    /// Downloads one file into `dest_dir`, unless a file of the same name and size is
    /// already there. Returns the local path and whether it was downloaded.
    ///
    /// `progress` is called with the bytes received so far; `cancel` aborts.
    pub fn download(
        &self,
        file: &RemoteFile,
        dest_dir: &Path,
        cancel: &AtomicBool,
        mut progress: impl FnMut(u64),
    ) -> Result<(PathBuf, bool)> {
        // Remote names come from the server: never let them escape the destination.
        if file.name.contains(['/', '\\']) || file.name.starts_with('.') {
            bail!("refusing suspicious file name {:?}", file.name);
        }
        let dest = dest_dir.join(&file.name);
        if fs::metadata(&dest).is_ok_and(|m| m.len() == file.size) {
            return Ok((dest, false));
        }
        fs::create_dir_all(dest_dir).with_context(|| format!("creating {}", dest_dir.display()))?;

        let remote_path = format!("{}/{}", self.target.dir.trim_end_matches('/'), file.name);
        let mut remote = self.sftp.open(Path::new(&remote_path)).with_context(|| format!("opening {remote_path}"))?;
        let partial = dest.with_extension("part");
        let result = (|| {
            let mut out = fs::File::create(&partial)?;
            let mut buf = vec![0u8; 32 * 1024];
            let mut total = 0u64;
            loop {
                if cancel.load(Ordering::Relaxed) {
                    bail!("cancelled");
                }
                let n = remote.read(&mut buf).with_context(|| format!("reading {remote_path}"))?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n])?;
                total += n as u64;
                progress(total);
            }
            out.flush()?;
            // Keep the OXE's timestamp, so the local copy shows when the file was written.
            if file.mtime > 0 {
                out.set_modified(UNIX_EPOCH + Duration::from_secs(file.mtime as u64))?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            let _ = fs::remove_file(&partial);
            return Err(e);
        }
        fs::rename(&partial, &dest).with_context(|| format!("saving {}", dest.display()))?;
        Ok((dest, true))
    }

    pub fn disconnect(self) {
        let _ = self.session.disconnect(None, "bye", None);
    }
}

fn login(session: &Session, user: &str, password: &str) -> Result<()> {
    let methods = session.auth_methods(user).unwrap_or("password");
    if methods.split(',').any(|m| m == "password") {
        session.userauth_password(user, password).context("login failed (user name or password wrong?)")?;
    } else if methods.split(',').any(|m| m == "keyboard-interactive") {
        struct Answer<'a>(&'a str);
        impl ssh2::KeyboardInteractivePrompt for Answer<'_> {
            fn prompt<'b>(&mut self, _: &str, _: &str, prompts: &[ssh2::Prompt<'b>]) -> Vec<String> {
                prompts.iter().map(|_| self.0.to_string()).collect()
            }
        }
        session
            .userauth_keyboard_interactive(user, &mut Answer(password))
            .context("login failed (user name or password wrong?)")?;
    } else {
        bail!("server does not allow password login (offers: {methods})");
    }
    if !session.authenticated() {
        bail!("login failed");
    }
    Ok(())
}

/// OpenSSH-style SHA256 fingerprint of the server's host key, e.g. `SHA256:abc…`.
pub fn fingerprint(session: &Session) -> String {
    session.host_key_hash(HashType::Sha256).map_or_else(|| "unknown".into(), |h| format!("SHA256:{}", base64(h)))
}

fn verify_host_key(session: &Session, target: &Target, known_hosts: &Path, trust: TrustNewKey) -> Result<()> {
    let (key, key_type) = session.host_key().ok_or_else(|| anyhow!("server sent no host key"))?;
    let mut hosts = session.known_hosts()?;
    if known_hosts.exists() {
        hosts
            .read_file(known_hosts, KnownHostFileKind::OpenSSH)
            .with_context(|| format!("reading {}", known_hosts.display()))?;
    }
    match hosts.check_port(&target.host, target.port, key) {
        CheckResult::Match => Ok(()),
        CheckResult::Mismatch => Err(HostKeyProblem::Changed { fingerprint: fingerprint(session) }.into()),
        CheckResult::NotFound if trust == TrustNewKey::Ask => {
            Err(HostKeyProblem::Unknown { fingerprint: fingerprint(session) }.into())
        }
        CheckResult::NotFound => {
            let name =
                if target.port == 22 { target.host.clone() } else { format!("[{}]:{}", target.host, target.port) };
            hosts.add(&name, key, "added by ipview", key_type.into())?;
            if let Some(dir) = known_hosts.parent() {
                fs::create_dir_all(dir)?;
            }
            hosts
                .write_file(known_hosts, KnownHostFileKind::OpenSSH)
                .with_context(|| format!("writing {}", known_hosts.display()))?;
            Ok(())
        }
        CheckResult::Failure => bail!("could not check the host key"),
    }
}

/// Standard base64 without padding, as OpenSSH prints fingerprints.
fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, &b)| acc | (b as u32) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// Per-user configuration directory for ipview (known_hosts lives here).
pub fn config_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default().join("ipview")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_default()
            .join("ipview")
    }
}

pub fn known_hosts_path() -> PathBuf {
    config_dir().join("known_hosts")
}

/// Default download folder: `Downloads/IPView/<host>` in the user's home.
pub fn default_download_dir(host: &str) -> PathBuf {
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_default();
    let safe: String =
        host.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
    home.join("Downloads").join("IPView").join(safe)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(name: &str, mtime: i64) -> RemoteFile {
        RemoteFile { name: name.into(), size: 100, mtime }
    }

    #[test]
    fn selection() {
        let now = 1_000_000;
        let files = [f("IPA.DAT", now - 50 * 3600), f("IPC.DAT", now - 1800), f("IPB.DAT", now - 5 * 3600)];
        let names = |v: Vec<RemoteFile>| v.into_iter().map(|f| f.name).collect::<Vec<_>>();
        assert_eq!(names(select(&files, Selection::LastFiles(2), now)), ["IPB.DAT", "IPC.DAT"]);
        assert_eq!(names(select(&files, Selection::LastFiles(10), now)).len(), 3);
        assert_eq!(names(select(&files, Selection::LastHours(1), now)), ["IPC.DAT"]);
        assert_eq!(names(select(&files, Selection::LastHours(6), now)), ["IPB.DAT", "IPC.DAT"]);
        assert_eq!(names(select(&files, Selection::LastDays(3), now)).len(), 3);
        assert_eq!(names(select(&files, Selection::LastDays(1), now)), ["IPB.DAT", "IPC.DAT"]);
    }

    #[test]
    fn ticket_names() {
        assert!(is_remote_ticket_name("IPBANHI.DAT"));
        assert!(is_remote_ticket_name("ipbanhi.dat"));
        assert!(!is_remote_ticket_name("TAXBANHI.DAT"));
        assert!(!is_remote_ticket_name("new.ipv"));
    }

    #[test]
    fn base64_matches_openssh_style() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg");
        assert_eq!(base64(b"fo"), "Zm8");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn download_dir_is_sanitized() {
        let d = default_download_dir("10.0.0.1:22/../x");
        assert!(d.ends_with("Downloads/IPView/10.0.0.1_22_.._x") || d.ends_with("Downloads\\IPView\\10.0.0.1_22_.._x"));
    }
}
