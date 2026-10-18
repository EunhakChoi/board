//! The server's secret key (signs preview tokens; later encrypts stored secrets).
//!
//! `BOARD_SECRET_KEY` (64 hex characters) wins; otherwise `data/secret.key` is created on first
//! run with mode 0600 and reused.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

pub fn load(data_dir: &Path, env: Option<String>) -> Result<[u8; 32], String> {
    if let Some(hex) = env {
        return decode(hex.trim())
            .ok_or_else(|| "BOARD_SECRET_KEY must be 64 hex characters".into());
    }
    let file = data_dir.join("secret.key");
    match fs::read(&file) {
        Ok(bytes) => bytes
            .try_into()
            .map_err(|_| format!("{} is not a 32-byte key", file.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut key = [0u8; 32];
            getrandom::fill(&mut key).map_err(|e| format!("no randomness: {e}"))?;
            let mut out = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&file)
                .map_err(|e| format!("cannot create {}: {e}", file.display()))?;
            out.write_all(&key)
                .map_err(|e| format!("cannot write {}: {e}", file.display()))?;
            Ok(key)
        }
        Err(error) => Err(format!("cannot read {}: {error}", file.display())),
    }
}

fn decode(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn creates_once_then_reuses() {
        let dir = std::env::temp_dir().join(format!("board-secret-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let first = load(&dir, None).unwrap();
        assert_eq!(load(&dir, None).unwrap(), first);
        let mode = fs::metadata(dir.join("secret.key"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn env_key_wins_and_is_checked() {
        let dir = Path::new("/nonexistent");
        assert_eq!(load(dir, Some("ab".repeat(32))).unwrap(), [0xab; 32]);
        assert!(load(dir, Some("xyz".into())).is_err());
    }
}
