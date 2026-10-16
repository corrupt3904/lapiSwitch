//! Publish a dump only after the complete transfer has been synced.
use super::{EdlError, Result};
use std::{fs::File, path::Path};

pub(super) fn write_dump(
    output: &Path,
    expected_bytes: u64,
    write: impl FnOnce(&mut File) -> Result<()>,
) -> Result<()> {
    if expected_bytes == 0 {
        return Err(EdlError::Session("Dump size must be nonzero".into()));
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    write(temporary.as_file_mut())?;
    let actual = temporary.as_file().metadata()?.len();
    if actual != expected_bytes {
        return Err(EdlError::Session(format!(
            "Incomplete dump: expected {expected_bytes} bytes, received {actual}"
        )));
    }
    temporary.as_file().sync_all()?;
    temporary.persist(output).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn failed_and_short_dumps_preserve_previous_backup() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("boot.img");
        for existing in [false, true] {
            if existing {
                std::fs::write(&out, b"previous").unwrap();
            }
            for fail in [false, true] {
                assert!(
                    write_dump(&out, 8, |file| {
                        file.write_all(b"part")?;
                        if fail {
                            return Err(EdlError::Session("disconnected".into()));
                        }
                        Ok(())
                    })
                    .is_err()
                );
                if existing {
                    assert_eq!(std::fs::read(&out).unwrap(), b"previous");
                } else {
                    assert!(!out.exists());
                }
                assert_eq!(
                    std::fs::read_dir(dir.path()).unwrap().count(),
                    usize::from(existing)
                );
            }
        }
        write_dump(&out, 8, |file| {
            file.write_all(b"complete")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"complete");
    }
}
