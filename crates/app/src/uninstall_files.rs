//! File ownership rules shared by uninstall planning and execution.

use std::io;
use std::path::{Path, PathBuf};

const FILES: &[&str] = &[
    "ram-control-center.exe",
    "ram-control-center-uninstaller.exe",
    "neo-control.exe",
    "boot-profile.ini",
    "boot-agent.log",
    "driver-install.log",
    "flexi-recovery.marker",
    "migration/boot-agent.log",
    "migration/driver-install.log",
    "drivers/acpibridge/AcpiBridge.inf",
    "drivers/acpibridge/AcpiBridge.sys",
    "drivers/acpibridge/acpibridge.cat",
    "drivers/hkkbdfltr/HKKbdFltr.inf",
    "drivers/hkkbdfltr/HKKbdFltr.sys",
    "drivers/hkkbdfltr/hkkbdfltr.cat",
    "drivers/hkkbdfltr/HKFltrInstaller.exe",
];
const DIRECTORIES: &[&str] = &[
    "drivers/acpibridge",
    "drivers/hkkbdfltr",
    "drivers",
    "migration",
    "",
];

#[derive(Debug, Default)]
pub(crate) struct CleanupReport {
    pub(crate) removed: Vec<PathBuf>,
    pub(crate) retained: Vec<PathBuf>,
    pub(crate) failures: Vec<(PathBuf, io::Error)>,
}

pub(crate) fn plan(root: &Path) -> io::Result<Vec<PathBuf>> {
    validate_directory(root)?;
    let mut files = Vec::new();
    for relative in FILES {
        let path = root.join(relative);
        validate_parents(root, &path)?;
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !is_redirect(&metadata) => files.push(path),
            Ok(_) => {
                return Err(io::Error::other(format!(
                    "expected an owned regular file: {}",
                    path.display()
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(files)
}

/// Removes only named application files. Unknown files and recovery backups
/// keep their containing directories alive and are included in the report.
pub(crate) fn remove(root: &Path) -> io::Result<CleanupReport> {
    let files = plan(root)?;
    let mut report = CleanupReport::default();
    for file in files {
        // Recheck the ownership boundary immediately before each deletion.
        validate_parents(root, &file)?;
        let metadata = std::fs::symlink_metadata(&file)?;
        if !metadata.is_file() || is_redirect(&metadata) {
            return Err(io::Error::other(format!(
                "file changed during cleanup: {}",
                file.display()
            )));
        }
        match std::fs::remove_file(&file) {
            Ok(()) => report.removed.push(file),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => report.failures.push((file, error)),
        }
    }
    for relative in DIRECTORIES {
        let directory = root.join(relative);
        validate_directory(&directory)?;
        if let Err(error) = std::fs::remove_dir(&directory) {
            if error.kind() == io::ErrorKind::NotFound {
                continue;
            }
            match std::fs::read_dir(&directory) {
                Ok(entries) => {
                    for entry in entries {
                        let path = entry?.path();
                        if !report.retained.contains(&path) {
                            report.retained.push(path);
                        }
                    }
                }
                Err(_) => report.failures.push((directory, error)),
            }
        }
    }
    Ok(report)
}

fn validate_parents(root: &Path, file: &Path) -> io::Result<()> {
    let mut current = file.parent();
    while let Some(directory) = current {
        validate_directory(directory)?;
        if directory == root {
            return Ok(());
        }
        current = directory.parent();
    }
    Err(io::Error::other(
        "file is outside the application directory",
    ))
}

fn validate_directory(directory: &Path) -> io::Result<()> {
    // Check all ancestors too: a junction above the owned folder changes the
    // deletion target just as a junction inside it does.
    for path in directory.ancestors() {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if !metadata.is_dir() || is_redirect(&metadata) => {
                return Err(io::Error::other(format!(
                    "directory redirects or is not a directory: {}",
                    path.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn is_redirect(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "ram-cleanup-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn planning_preserves_files_and_removal_retains_unknown_files_and_recovery() {
        let fixture = Fixture::new();
        let root = &fixture.0;
        std::fs::write(root.join("neo-control.exe"), b"owned").unwrap();
        std::fs::write(root.join("notes.txt"), b"user file").unwrap();
        std::fs::create_dir(root.join("repair-backup")).unwrap();
        std::fs::write(root.join("repair-backup/input.reg"), b"recovery").unwrap();
        assert_eq!(plan(root).unwrap(), [root.join("neo-control.exe")]);
        assert!(root.join("neo-control.exe").exists());
        let result = remove(root).unwrap();
        assert_eq!(result.removed, [root.join("neo-control.exe")]);
        assert!(result.failures.is_empty());
        assert!(result.retained.contains(&root.join("repair-backup")));
        assert_eq!(std::fs::read(root.join("notes.txt")).unwrap(), b"user file");
        assert!(root.join("repair-backup/input.reg").exists());
    }
    #[test]
    fn deletes_payload_copies_and_empty_directories() {
        let fixture = Fixture::new();
        let root = fixture.0.join("application");
        std::fs::create_dir_all(root.join("drivers/acpibridge")).unwrap();
        std::fs::write(
            root.join("drivers/acpibridge/AcpiBridge.sys"),
            b"local copy",
        )
        .unwrap();
        let result = remove(&root).unwrap();
        assert_eq!(result.removed.len(), 1);
        assert!(result.failures.is_empty());
        assert!(!root.exists());
        assert!(remove(&root).unwrap().failures.is_empty());
    }
    #[test]
    fn directory_with_executable_name_is_preserved() {
        let fixture = Fixture::new();
        std::fs::create_dir(fixture.0.join("neo-control.exe")).unwrap();
        assert!(remove(&fixture.0).is_err());
        assert!(fixture.0.join("neo-control.exe").is_dir());
    }
    #[cfg(unix)]
    #[test]
    fn rejects_redirected_payload_and_ancestors() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let other = Fixture::new();
        std::fs::write(other.0.join("AcpiBridge.sys"), b"outside").unwrap();
        std::fs::create_dir(fixture.0.join("drivers")).unwrap();
        symlink(&other.0, fixture.0.join("drivers/acpibridge")).unwrap();
        assert!(remove(&fixture.0).is_err());
        assert!(other.0.join("AcpiBridge.sys").exists());
        let alias = fixture.0.join("alias");
        symlink(&other.0, &alias).unwrap();
        assert!(plan(&alias.join("application")).is_err());
    }
}
