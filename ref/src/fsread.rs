//! C-431 (almide/almide#4201): the fs whole-file readers read a REGULAR file
//! of at most 1 GiB. The judge's own statement of the rule, written from the
//! contract (no almide code): open read-only and O_NONBLOCK (a FIFO with no
//! writer must not block the open), judge the OPENED descriptor's kind, then
//! read through `take(cap + 1)`.
//!
//! - neither a regular file nor a directory (a device, FIFO, socket) errs
//!   `Not a regular file`;
//! - a regular file whose size, or whose bytes actually read, exceed the cap
//!   errs `File too large (the read limit is 1073741824 bytes)`;
//! - a directory is read like any file, so it keeps the read's own EISDIR.
//!
//! The texts are the io error's Display; the call wraps them in
//! `fs.<call>("<path>"): ` as every other fs error.

use std::io::Read;

/// The read limit, 1 GiB.
pub const READ_MAX_BYTES: u64 = 1_073_741_824;
const NOT_REGULAR: &str = "Not a regular file";
const TOO_LARGE: &str = "File too large (the read limit is 1073741824 bytes)";

#[cfg(target_os = "linux")]
const O_NONBLOCK: i32 = 0o4000;
#[cfg(target_os = "macos")]
const O_NONBLOCK: i32 = 0x4;

fn open(p: &str) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut o, O_NONBLOCK);
    o.open(p)
}

fn too_large() -> std::io::Error {
    std::io::Error::other(TOO_LARGE)
}

/// The whole regular file at `p`, at most [`READ_MAX_BYTES`].
pub fn read(p: &str) -> std::io::Result<Vec<u8>> {
    let f = open(p)?;
    let m = f.metadata()?;
    if !m.is_file() && !m.is_dir() {
        return Err(std::io::Error::other(NOT_REGULAR));
    }
    if m.is_file() && m.len() > READ_MAX_BYTES {
        return Err(too_large());
    }
    let mut out = Vec::new();
    f.take(READ_MAX_BYTES + 1).read_to_end(&mut out)?;
    if out.len() as u64 > READ_MAX_BYTES {
        return Err(too_large());
    }
    Ok(out)
}

/// [`read`] as UTF-8 text, with `read_to_string`'s invalid-data error.
pub fn read_to_string(p: &str) -> std::io::Result<String> {
    String::from_utf8(read(p)?).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        )
    })
}
