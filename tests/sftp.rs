//! End-to-end SFTP test against `scripts/test_sftp_server.py`.
//!
//! Set IPVIEW_SFTP_TEST_PYTHON to a Python interpreter with `asyncssh` installed
//! to run it; otherwise the test is skipped.

#![cfg(feature = "sftp")]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, UNIX_EPOCH};

use ipview::remote::{self, Connection, HostKeyProblem, Selection, Target, TrustNewKey};

struct TestServer {
    child: Child,
    port: u16,
}

impl TestServer {
    fn start(python: &str, root: &Path, key: &Path) -> TestServer {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/test_sftp_server.py");
        let mut child = Command::new(python)
            .arg(script)
            .arg(root)
            .arg(key)
            .stdout(Stdio::piped())
            .spawn()
            .expect("starting test server");
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let port = line.trim().parse().unwrap_or_else(|_| panic!("server said {line:?}"));
        TestServer { child, port }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ipview-sftp-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_file(dir: &Path, name: &str, content: &[u8], mtime: u64) {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(mtime))
        .unwrap();
}

#[test]
fn download_flow() {
    let Ok(python) = std::env::var("IPVIEW_SFTP_TEST_PYTHON") else {
        eprintln!("IPVIEW_SFTP_TEST_PYTHON not set, skipping");
        return;
    };
    let base = temp_dir("root");
    let account = base.join("usr4/account");
    std::fs::create_dir_all(&account).unwrap();
    let now = remote::now() as u64;
    write_file(&account, "IPOLD.DAT", b"\x01\x00\x04old!", now - 3 * 86_400);
    write_file(&account, "IPNEW.DAT", b"\x01\x00\x04new!", now - 600);
    write_file(&account, "TAXNEW.DAT", b"charging", now - 600);

    let key = base.join("host_key");
    let known_hosts = base.join("known_hosts");
    let downloads = base.join("downloads");
    let server = TestServer::start(&python, &base, &key);
    let target = Target { host: "127.0.0.1".into(), port: server.port, ..Target::default() };

    // First contact: the fingerprint must be confirmed.
    let err = Connection::open(&target, "secret", &known_hosts, TrustNewKey::Ask).err().unwrap();
    let Some(HostKeyProblem::Unknown { fingerprint }) = err.downcast_ref::<HostKeyProblem>() else {
        panic!("expected unknown host key, got {err:#}");
    };
    assert!(fingerprint.starts_with("SHA256:"));

    // Accepting stores the key; afterwards no confirmation is needed.
    Connection::open(&target, "secret", &known_hosts, TrustNewKey::Accept).unwrap().disconnect();
    let conn = Connection::open(&target, "secret", &known_hosts, TrustNewKey::Ask).unwrap();

    let files = conn.list().unwrap();
    let mut names: Vec<_> = files.iter().map(|f| f.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["IPNEW.DAT", "IPOLD.DAT"], "only IP*.DAT files");

    let chosen = remote::select(&files, Selection::LastDays(1), remote::now());
    assert_eq!(chosen.len(), 1);
    assert_eq!(chosen[0].name, "IPNEW.DAT");

    let cancel = AtomicBool::new(false);
    let mut seen = 0;
    let (path, fetched) = conn.download(&chosen[0], &downloads, &cancel, |n| seen = n).unwrap();
    assert!(fetched);
    assert_eq!(seen, 7);
    assert_eq!(std::fs::read(&path).unwrap(), b"\x01\x00\x04new!");
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_secs();
    assert_eq!(mtime, now - 600, "remote timestamp kept");

    // Same name and size: skipped.
    let (_, fetched) = conn.download(&chosen[0], &downloads, &cancel, |_| {}).unwrap();
    assert!(!fetched);
    conn.disconnect();

    // Wrong password.
    let err = Connection::open(&target, "wrong", &known_hosts, TrustNewKey::Ask).err().unwrap();
    assert!(format!("{err:#}").contains("login failed"), "{err:#}");

    // A new host key on the same address is refused, even when accepting new keys.
    drop(server);
    std::fs::remove_file(&key).unwrap();
    let server = TestServer::start(&python, &base, &key);
    let moved = Target { port: server.port, ..target.clone() };
    std::fs::write(
        &known_hosts,
        std::fs::read_to_string(&known_hosts).unwrap().replace(&target.port.to_string(), &moved.port.to_string()),
    )
    .unwrap();
    let err = Connection::open(&moved, "secret", &known_hosts, TrustNewKey::Accept).err().unwrap();
    assert!(matches!(err.downcast_ref::<HostKeyProblem>(), Some(HostKeyProblem::Changed { .. })), "{err:#}");

    let _ = std::fs::remove_dir_all(&base);
}
