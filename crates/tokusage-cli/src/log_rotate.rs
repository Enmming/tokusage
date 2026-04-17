//! Tiny log rotator: if `submit.log` exceeds `MAX_BYTES`, rename it to
//! `submit.log.1` (clobbering any prior .1), and start fresh. Called on
//! every `tokusage submit` invocation so launchd's long-lived appends
//! don't fill the disk.

use std::fs;
use std::path::Path;

/// Rotate once the log passes 10 MiB. Large enough for a few months of
/// 2-hour runs, small enough to never bloat the user's disk.
const MAX_BYTES: u64 = 10 * 1024 * 1024;

pub fn rotate_if_needed(log: &Path) {
    let Ok(meta) = fs::metadata(log) else {
        return;
    };
    if meta.len() <= MAX_BYTES {
        return;
    }
    let rotated = log.with_extension(
        log.extension()
            .and_then(|e| e.to_str())
            .map(|s| format!("{}.1", s))
            .unwrap_or_else(|| "1".to_string()),
    );
    let _ = fs::remove_file(&rotated);
    let _ = fs::rename(log, &rotated);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn no_op_when_under_threshold() {
        let tmp = tempfile::TempDir::new().unwrap();
        let log = tmp.path().join("submit.log");
        fs::write(&log, b"small").unwrap();
        rotate_if_needed(&log);
        assert!(log.exists());
        assert!(!log.with_extension("log.1").exists());
    }

    #[test]
    fn rotates_when_over_threshold() {
        let tmp = tempfile::TempDir::new().unwrap();
        let log = tmp.path().join("submit.log");
        let mut f = fs::File::create(&log).unwrap();
        // Write 10 MiB + 1 byte to cross the threshold.
        let chunk = vec![b'x'; 1024];
        for _ in 0..(10 * 1024) {
            f.write_all(&chunk).unwrap();
        }
        f.write_all(b"!").unwrap();
        drop(f);

        rotate_if_needed(&log);
        assert!(!log.exists(), "original log should be gone");
        let rotated = log.with_extension("log.1");
        assert!(rotated.exists(), "rotated log should be created");
    }

    #[test]
    fn rotation_clobbers_prior_rotated_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let log = tmp.path().join("submit.log");
        let rotated = log.with_extension("log.1");

        // Pre-existing .1 from an earlier rotation.
        fs::write(&rotated, b"old content").unwrap();

        // Current log exceeds threshold.
        let big = vec![b'y'; MAX_BYTES as usize + 1];
        fs::write(&log, &big).unwrap();

        rotate_if_needed(&log);
        let new_rotated = fs::read(&rotated).unwrap();
        assert_eq!(new_rotated.len(), MAX_BYTES as usize + 1, "should be the new large file");
    }
}
