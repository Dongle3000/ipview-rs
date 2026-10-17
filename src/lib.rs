//! Reader and analyzer for Alcatel-Lucent OmniPCX Enterprise VoIP tickets
//! (`IP*.DAT` files and IPView `.ipv` files).
//!
//! ```no_run
//! let tickets = ipview::file::read("IPBANHI.DAT".as_ref())?;
//! for t in &tickets {
//!     println!("{:?} MOS {:?}", t.value(1), t.metrics().mos);
//! }
//! # anyhow::Ok(())
//! ```

pub mod compress;
pub mod emodel;
pub mod export;
pub mod fields;
pub mod file;
pub mod filter;
pub mod stats;
pub mod ticket;

pub use ticket::{Metrics, Ticket, Value};
