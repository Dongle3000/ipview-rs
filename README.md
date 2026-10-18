# ipview-rs

A Rust rewrite of **IPView 2.0.1** (Sven Fabricius, 2007), the unmaintained Windows
analyzer for Alcatel-Lucent OmniPCX Enterprise (OXE) VoIP quality tickets (`IP*.DAT`).

It reads the same files, computes the same derived metrics (packet loss, round-trip
delay, jitter delay, E-model MOS), and writes `.ipv` files the original can still open.

## Build

```sh
cargo build --release        # binary: target/release/ipview
cargo test                   # unit tests
IPVIEW_SAMPLES=~/ipview cargo test --release   # also check a directory of real files
IPVIEW_SNAPSHOT_DIR=/tmp/shots cargo test --bin ipview-gui   # GUI tests, saving rendered images
IPVIEW_SFTP_TEST_PYTHON=python3 cargo test   # SFTP tests against scripts/test_sftp_server.py (needs asyncssh)
```

## Usage

Paths can be files or directories (non-recursive; `IP*.DAT` and `*.ipv` are picked up).

```sh
ipview stats ~/ipview                                  # overview: MOS bands, codecs, equipment
ipview list  ~/ipview --sort mos -n 20                 # 20 worst calls (IPView's default columns)
ipview list  ~/ipview -c end_time,local_ip,distant_ip,codec,loss_pct,mos
ipview show  ~/ipview -w "mos<3" -n 1                  # every field of a ticket, grouped
ipview list  ~/ipview --between 192.168.91.13 192.168.91.50
ipview export ~/ipview -o tickets.csv                  # all fields; --delimiter ';' for Excel (DE)
ipview export ~/ipview -f json -w "equipment=NOE-IP"
ipview save  ~/ipview -w "loss_pct>1" -o bad.ipv       # IPView-compatible subset
ipview fields                                          # field IDs and keys
ipview fetch 10.0.0.10 --last-days 2                   # download IP*.DAT from the OXE via SFTP
```

### Downloading from the OXE

`ipview fetch HOST` connects over SSH/SFTP (user `adfexc`, folder `/usr4/account` by default;
`--user`, `--dir`, `--port` to change), asks for the password (or reads `IPVIEW_SSH_PASSWORD`),
and downloads the selected `IP*.DAT` files: `--last-files N`, `--last-days N` (default 1) or
`--last-hours N`, by file modification time. Files go to `Downloads/IPView/<host>` unless `-o`
is given; files already present with the same size are skipped, so repeated runs only fetch new
ones. `--list` only shows what would be downloaded.

The server's host key is checked like OpenSSH does: on the first connection its SHA256
fingerprint is shown for confirmation (or `--accept-host-key`), then stored in `known_hosts`
under `%APPDATA%\ipview` (Windows) or `~/.config/ipview`. A changed key is refused.
In the GUI the same is available under File > Download from OXE (SFTP); the password is not saved.

Filters (`-w/--where`, repeatable, AND by default, `--any` for OR) use a field key, ID or
name, an operator `= != < <= > >= ~ !~` (`~` = contains), and a value. Timestamps compare
by prefix (`-w "end_time>=2026-09-01"`), enumerations by name or raw code (`-w codec=8`).
Quote filters containing `<` or `>` so the shell doesn't treat them as redirections.

## GUI

```sh
cargo run --release --bin ipview-gui -- ~/ipview     # or start it and use File > Open / drag and drop
```

Filter panel on the left (MOS, loss, equipment, codec, phone number, IP or IP pair, period,
plus free field conditions with AND/OR), sortable ticket grid with MOS/loss coloring, and
all fields of the selected ticket on the right with distributions as bar charts.
View > Statistics summarizes the visible tickets. Right-click a cell to filter on its value.
Right-click a column header to sort ascending/descending, add a filter on that column (pick an
operator, type a value, Enter), remove its filters, move or hide it. The Columns button above the
table (also in the header menu) picks columns by group; View > Columns also reorders them.
File menu: open/add files or folders, save visible tickets as `.ipv`, export CSV/JSON.
Keys as in IPView: F3 open, F6 clear filters, F8 clear database; arrow keys move the selection.
Columns, sort order, filters, the CSV delimiter and the download settings (not the password) are
remembered between sessions.

The interface is in English or German. By default it follows the system language (German for
any `de-*` locale); View > Language switches it. Error details from the network or files may
still be in English.

The CLI alone builds without GUI dependencies: `cargo build --release --no-default-features --bin ipview`.

Under WSL, if the window fails with `NoGlutinConfigs`, start it via X11:
`env -u WAYLAND_DISPLAY ipview-gui`.

## File format

Recovered by reverse-engineering IPView.exe (it is packed with a Yoda's Crypter-style
protector; the code was unpacked by emulation) and confirmed against real files.

**Container.** First two bytes `1F 9D`: Unix `compress` (LZW, `.Z`). First two bytes
`01 00`: plain ticket stream. `.ipv` files are plain streams.

**Stream.** A sequence of TLV entries; a new ticket starts at every entry with ID 1.

```
id      u16 little-endian
length  u8
value   `length` bytes, numbers big-endian; shorter numbers are zero-extended
```

**Fields.** Run `ipview fields` for the full list. Notable encodings:

| ID | Field | Encoding |
|----|-------|----------|
| 1 | End of communication | u32 Unix time (shown as UTC, as IPView did) |
| 6 | Equipment | 2 bytes: class, sub-type (1/2 = NOE-IP, 3/n = OXE, ...) |
| 8, 9 | Local / distant IP | u32 |
| 10, 11, 39 | Local / distant ID, MCDU | digits: bytes 1-9 are digits, 10 is "0"; zero padded |
| 15 | Codec | 0 G711A, 1 G711U, 2 G723 6.3, 3 G729A, 4 G723 5.3; 5 G722, 8 Opus (inferred) |
| 18 | Voice mode | 0x50 Idle ... 0x55 Headset |
| 27 | Round-trip delay distribution | 5 x u16: 0-40, 40-80, 80-150, 150-250, 250+ ms |
| 30 | Consecutive BFI | 10 x u16 |
| 32 | Jitter buffer depth | 10 x u32; bucket i = (i+1) frames |
| 38 | Software version | u16, shown as high.low byte |
| 61 | BFI over 200 ms | 5 x u16 |
| 62 | Consecutive RTP lost | 5 x u16: 1, 2, 3, 4, 5+ |

**Derived values** (IPView's pseudo-fields 500-503):

* loss % = 100 x lost (24) / (received (22) + lost)
* round-trip = mean of field 27 using 20/60/115/200 ms and min(max delay (28), 500) ms
* jitter delay = mean of field 32 with bucket i weighted (i+1) x received framing (20)
* MOS = ITU-T G.107 E-model with per-codec Ie/Bpl and delay, phone-type-dependent loudness
  ratings, and a packet-loss estimate from BFI counters (see `src/ticket.rs`)

**Wideband (not in IPView).** G.722 calls are scored with the wideband E-model, ITU-T G.107.1
(06/2019 with Corrigendum 1), using Ie,wb = 5 and Bpl,wb = 7.1 from G.113 (09/2024) Table IV.4
(diotic listening, high-quality PLC). Their `mos` is MOS-CQEW (R/1.29 on the narrowband curve).
Field 504 `r_wb` gives every call's R on the extended 0-129 scale so codecs can be compared:
narrowband codecs use Ie,wb = Ie + 35.8 (G.113 Appendix IV) with their narrowband Bpl.
For reference, a perfect G.711 call scores about 93 and a perfect G.722 call about 124.

## Known differences and limitations

* Newer OXE releases emit codec IDs IPView never knew. 5 and 8 are shown as G722 and Opus:
  inferred from the data (5 uses the same 5 ms DSP framing as G.711 on NOE phones, 8 uses
  20 ms frame-codec framing), not from documentation. Opus gets no MOS: G.113 has no planning
  values for it. The G.722 assumptions (PLC type, G.711-like framing and delay) are unverified.
* Some NOE tickets carry implausible jitter-buffer counters (field 32, e.g. 0x2020 = two
  spaces; healthy tickets have at most ~4 counts per received packet, these over 1000).
  When the total exceeds 10 per packet, field 505 `jitter_corrupt` is set, jitter delay is
  left empty, and MOS uses the RTP packet-loss rate (burst ratio 1) instead of IPView's
  frame-loss estimate, which would make such calls look loss-free.
* IPView's lost-frame estimate for NOE phones overwrites instead of summing across the
  BFI-over-200ms buckets, so only the last bucket counts. This is reproduced deliberately so
  MOS values match; fix it in `ticket::mos` if you prefer the evident intent.
* Field 45 (minimum delay) uses 65535 for "no measurement"; it is shown as-is.
* Not ported: plain FTP download and the original's FTP address book (recent SFTP targets are
  remembered instead). SFTP has not yet been tried against a real OXE, only against a test server.
