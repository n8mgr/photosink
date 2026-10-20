use anyhow::{Context, Result};
use std::io::ErrorKind;
use std::path::Path;
use std::time::Duration;

#[derive(Debug)]
pub(super) struct RetryDelay(pub Duration);

impl std::fmt::Display for RetryDelay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "server requested a {} second retry delay",
            self.0.as_secs()
        )
    }
}

impl std::error::Error for RetryDelay {}

pub(super) fn download<T>(dest: &Path, attempt: impl FnMut() -> Result<T>) -> Result<T> {
    run(dest, attempt, std::thread::sleep)
}

fn remove_partial(dest: &Path) -> Result<()> {
    match std::fs::remove_file(dest) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("remove partial download {}", dest.display())),
    }
}

fn run<T>(
    dest: &Path,
    mut attempt: impl FnMut() -> Result<T>,
    mut sleep: impl FnMut(Duration),
) -> Result<T> {
    remove_partial(dest)?;
    for retry in 0..=3 {
        match attempt() {
            Ok(value) => return Ok(value),
            Err(error) => {
                remove_partial(dest)?;
                if retry == 3 {
                    return Err(error).context("download failed after 3 retries");
                }
                let delay = Duration::from_secs(1 << retry);
                sleep(
                    error
                        .downcast_ref::<RetryDelay>()
                        .map_or(delay, |wait| delay.max(wait.0)),
                );
            }
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::bail;

    #[test]
    fn retries_partial_downloads_and_keeps_success() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("download");
        std::fs::write(&dest, "stale").unwrap();
        let mut attempts = 0;
        let mut waits = Vec::new();
        let value = run(
            &dest,
            || {
                assert!(!dest.exists());
                attempts += 1;
                if attempts < 4 {
                    std::fs::write(&dest, "partial")?;
                    bail!("connection lost");
                }
                std::fs::write(&dest, "complete")?;
                Ok(42)
            },
            |wait| waits.push(wait.as_secs()),
        )
        .unwrap();
        assert_eq!(value, 42);
        assert_eq!(attempts, 4);
        assert_eq!(waits, [1, 2, 4]);
        assert_eq!(std::fs::read_to_string(dest).unwrap(), "complete");
    }

    #[test]
    fn stops_after_three_retries_and_removes_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("download");
        let mut attempts = 0;
        let error = run::<()>(
            &dest,
            || {
                attempts += 1;
                std::fs::write(&dest, "partial")?;
                bail!("connection lost");
            },
            |_| {},
        )
        .unwrap_err();
        assert_eq!(attempts, 4);
        assert!(!dest.exists());
        assert_eq!(
            format!("{error:#}"),
            "download failed after 3 retries: connection lost"
        );
    }

    #[test]
    fn honors_server_retry_delay() {
        let dir = tempfile::tempdir().unwrap();
        let mut attempts = 0;
        let mut waits = Vec::new();
        run(
            &dir.path().join("download"),
            || {
                attempts += 1;
                if attempts == 1 {
                    return Err(
                        anyhow::anyhow!("throttled").context(RetryDelay(Duration::from_secs(30)))
                    );
                }
                Ok(())
            },
            |wait| waits.push(wait.as_secs()),
        )
        .unwrap();
        assert_eq!(waits, [30]);
    }

    #[test]
    fn successful_download_does_not_wait() {
        let dir = tempfile::tempdir().unwrap();
        run(
            &dir.path().join("download"),
            || Ok(()),
            |_| panic!("unexpected retry"),
        )
        .unwrap();
    }
}
