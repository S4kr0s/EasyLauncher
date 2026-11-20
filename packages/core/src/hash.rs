use sha1::{Digest, Sha1};
use std::io::Read;
use std::path::Path;

pub fn sha1_bytes(data: &[u8]) -> String {
    hex::encode(Sha1::digest(data))
}

/// SHA-1 of a file, or `None` if it does not exist.
pub fn sha1_file(path: &Path) -> anyhow::Result<Option<String>> {
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if f.metadata()?.is_dir() {
        return Ok(None);
    }
    let mut h = Sha1::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(Some(hex::encode(h.finalize())))
}
