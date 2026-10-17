//! Checks against real ticket files. Set IPVIEW_SAMPLES to a directory of
//! IP*.DAT files to run it; without it the test is skipped.

use std::path::PathBuf;

use ipview::{file, ticket};

#[test]
fn real_files_parse_cleanly() {
    let Some(dir) = std::env::var_os("IPVIEW_SAMPLES") else {
        eprintln!("IPVIEW_SAMPLES not set, skipping");
        return;
    };
    let paths = file::expand(&[PathBuf::from(dir)]).unwrap();
    assert!(!paths.is_empty(), "no ticket files found");

    let mut total = 0;
    for path in &paths {
        let data = std::fs::read(path).unwrap();
        let tickets = file::parse(&data).unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        let stream: Vec<u8> = tickets.iter().flat_map(|t| t.raw().to_vec()).collect();

        // Every ticket starts with field 1 and the records cover the stream exactly.
        for t in &tickets {
            assert_eq!(&t.raw()[..2], &[1, 0], "{}", path.display());
        }
        assert_eq!(ticket::split(&stream), tickets);
        if file::detect(&data) == Some(file::Format::Plain) {
            assert_eq!(stream, data);
        }

        for t in &tickets {
            let m = t.metrics();
            if let Some(mos) = m.mos {
                assert!((1.0..=4.5).contains(&mos), "{}: MOS {mos}", path.display());
            }
            if let Some(r) = m.r_wb {
                assert!(r <= 129.0, "{}: R {r}", path.display());
            }
            if let Some(loss) = m.loss_pct {
                assert!((0.0..=100.0).contains(&loss), "{}: loss {loss}", path.display());
            }
        }
        total += tickets.len();
    }
    eprintln!("{} files, {total} tickets", paths.len());
}
