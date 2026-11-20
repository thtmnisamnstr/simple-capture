use std::fs::File;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::{Error, Result};

/// Lower-case hex SHA-256 of a file, read in 64 KiB chunks.
pub fn sha256_file(p: &Path) -> Result<String> {
    let mut f = File::open(p).map_err(|e| Error::io_at(e, "open for sha256", p))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Error::io_at(e, "read for sha256", p)),
        };
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

/// Lower-case hex SHA-256.
pub fn sha256_bytes(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(char::from(DIGITS[usize::from(b >> 4)]));
        s.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    s
}
