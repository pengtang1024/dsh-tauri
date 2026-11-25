//! Stage an npm install in a sibling directory and swap it in atomically.
//!
//! Writing straight into the live `node_modules` would let a running server load
//! a half-replaced tree, and a failed install would leave the runtime broken.
//! The new tree is therefore built at `<runtime>.new-<pid>` — the same
//! filesystem, so `rename` is atomic — and only replaces the live directory once
//! it is complete. A failed swap restores the previous runtime.

use std::path::{Path, PathBuf};

/// A staged runtime tree, committed or discarded by `Drop`.
pub struct Staging {
    target: PathBuf,
    stage: PathBuf,
    committed: bool
}

impl Staging {
    /// Resolve the directory to replace (a valid symlink is followed) and create
    /// the staging directory next to it, clearing leftovers from a killed run.
    pub fn new(dir: &Path) -> Result<Self, String> {
        let target = match std::fs::symlink_metadata(dir) {
            Ok(meta) if meta.file_type().is_symlink() => match std::fs::canonicalize(dir) {
                Ok(real) if real.is_dir() => real,
                _ => {
                    eprintln!("[runtime] {} 是失效的软链接，安装后会用真实目录替换它", dir.display());
                    dir.to_path_buf()
                }
            },
            _ => dir.to_path_buf()
        };
        clean_stale(&target);
        let stage = sibling(&target, "new");
        remove(&stage).map_err(|error| format!("无法清理临时目录 {}：{error}", stage.display()))?;
        if let Some(parent) = stage.parent() {
            std::fs::create_dir_all(parent).map_err(|error| format!("无法创建 {}：{error}", parent.display()))?;
        }
        std::fs::create_dir_all(&stage).map_err(|error| format!("无法创建临时目录 {}：{error}", stage.display()))?;
        Ok(Self { target, stage, committed: false })
    }

    /// Where npm should install.
    pub fn path(&self) -> &Path {
        &self.stage
    }

    /// Replace the live runtime with the staged tree, then drop the old one.
    /// On failure the old runtime is moved back and the staged tree discarded.
    pub fn commit(mut self) -> Result<(), String> {
        let old = sibling(&self.target, "old");
        let _ = remove(&old);
        let existing = std::fs::symlink_metadata(&self.target).is_ok();
        if existing {
            std::fs::rename(&self.target, &old).map_err(|error| {
                format!("无法移走旧运行时 {}：{error}", self.target.display())
            })?;
        }
        if let Err(error) = std::fs::rename(&self.stage, &self.target) {
            if existing {
                let _ = std::fs::rename(&old, &self.target);
            }
            return Err(format!(
                "无法把新运行时移动到 {}：{error}（旧运行时已恢复）",
                self.target.display()
            ));
        }
        self.committed = true;
        eprintln!("[runtime] 已原子替换 {}", self.target.display());
        if existing {
            // The running server still resolves files through the old tree, so
            // it is kept until the next launch cleans it up.
            eprintln!("[runtime] 旧运行时保留至下次启动清理：{}", old.display());
        }
        Ok(())
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if !self.committed {
            let _ = remove(&self.stage);
        }
    }
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{suffix}-{}", std::process::id()));
    PathBuf::from(name)
}

/// Remove a file, directory or symlink; a missing path is not an error.
fn remove(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || meta.is_file() => std::fs::remove_file(path),
        Ok(_) => std::fs::remove_dir_all(path),
        Err(_) => Ok(())
    }
}

/// Remove `.new-*` / `.old-*` siblings left next to the installed runtime.
///
/// Called early at startup, before anything is installed or started: a SIGTERM
/// during an install leaves the staging tree behind, and a finished update keeps
/// the previous tree alive for the running server. Only the install directory's
/// own level is scanned, and failures are only logged.
pub fn clean_stale_runtime() {
    clean_stale(&crate::runtime::install_dir());
}

/// Remove `<name>.new-*` / `<name>.old-*` siblings left by an earlier run.
fn clean_stale(target: &Path) {
    let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else { return };
    let prefix = format!("{}.", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(parent) else { return };
    for entry in entries.flatten() {
        let entry_name = entry.file_name();
        let entry_name = entry_name.to_string_lossy();
        let stale = entry_name.starts_with(&prefix)
            && (entry_name.contains(".new-") || entry_name.contains(".old-"));
        if stale {
            eprintln!("[runtime] 清理上次运行留下的 {}", entry.path().display());
            let _ = remove(&entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{clean_stale, remove, sibling, Staging};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// Temp roots are unique per case, but a process-wide lock keeps the suite
    /// deterministic in case any shared state is added later.
    static SERIAL: Mutex<()> = Mutex::new(());
    static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A throwaway directory under std::env::temp_dir(); the real
    /// ~/.dsh/dsh-tauri/runtime is never touched. Removed on drop.
    struct TempCase {
        root: PathBuf,
    }

    impl TempCase {
        fn new(label: &str) -> Self {
            let seq = NEXT_CASE.fetch_add(1, Ordering::Relaxed);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "dsh-swap-test-{}-{}-{}-{}",
                std::process::id(), label, seq, nanos
            ));
            let _ = remove(&root); // guard against a crashed earlier run reusing the name
            fs::create_dir_all(&root).expect("create temp case root");
            Self { root }
        }

        fn target(&self) -> PathBuf {
            self.root.join("target")
        }
    }

    impl Drop for TempCase {
        fn drop(&mut self) {
            let _ = remove(&self.root);
        }
    }

    #[test]
    fn new_creates_staging_sibling_and_path_points_to_it() {
        let _guard = serial();
        let case = TempCase::new("new");
        let target = case.target();

        let staging = Staging::new(&target).expect("Staging::new must succeed");
        let expected = sibling(&target, "new");

        assert_eq!(staging.path(), expected.as_path(), "path() must be <target>.new-<pid>");
        assert!(staging.path().is_dir(), "staging directory must exist");
    }

    #[test]
    fn drop_without_commit_removes_staging_tree() {
        let _guard = serial();
        let case = TempCase::new("drop");
        let target = case.target();

        let stage = {
            let staging = Staging::new(&target).expect("Staging::new must succeed");
            let stage = staging.path().to_path_buf();
            fs::write(stage.join("partial.bin"), "half-installed").expect("write into staging");
            assert!(stage.is_dir());
            stage // staging dropped here without commit -> interrupted install
        };

        assert!(!stage.exists(), "aborted staging tree must be removed by Drop");
    }

    #[test]
    fn commit_swaps_in_new_tree_and_keeps_old_tree() {
        let _guard = serial();
        let case = TempCase::new("commit");
        let target = case.target();

        fs::create_dir_all(&target).expect("create old target");
        fs::write(target.join("old.txt"), "old payload").expect("write old payload");

        let staging = Staging::new(&target).expect("Staging::new must succeed");
        fs::write(staging.path().join("new.txt"), "new payload").expect("write new payload");
        staging.commit().expect("commit must succeed");

        let old = sibling(&target, "old");
        assert!(target.join("new.txt").is_file(), "new tree must be at target");
        assert!(!target.join("old.txt").exists(), "stale content must not remain at target");
        assert!(old.join("old.txt").is_file(), "old tree must survive at <target>.old-<pid>");
        assert_eq!(fs::read_to_string(old.join("old.txt")).unwrap(), "old payload");
        assert!(!sibling(&target, "new").exists(), "staging sibling must be gone after commit");
    }

    #[test]
    fn new_clears_stale_new_and_old_siblings_first() {
        let _guard = serial();
        let case = TempCase::new("leftover");
        let target = case.target();
        let root = &case.root;

        let stale_new = root.join(format!("target.new-{}", 41111));
        let stale_old = root.join(format!("target.old-{}", 42222));
        fs::create_dir_all(&stale_new).unwrap();
        fs::create_dir_all(&stale_old).unwrap();
        fs::write(stale_new.join("junk"), "x").unwrap();
        fs::write(stale_old.join("junk"), "x").unwrap();

        let staging = Staging::new(&target).expect("Staging::new must succeed");

        assert!(!stale_new.exists(), "leftover .new-* must be cleared");
        assert!(!stale_old.exists(), "leftover .old-* must be cleared");
        assert!(staging.path().is_dir(), "fresh staging dir must exist");
    }

    #[test]
    fn clean_stale_only_removes_matching_siblings() {
        let _guard = serial();
        let case = TempCase::new("clean");
        let target = case.target();
        let root = &case.root;

        let new_dir = root.join(format!("target.new-{}", 51111));
        let old_dir = root.join(format!("target.old-{}", 52222));
        let plain_sibling = root.join("target-backup");
        let unrelated_file = root.join("targets.txt");
        let other_prefix = root.join(format!("other.new-{}", 53333));

        fs::create_dir_all(&new_dir).unwrap();
        fs::create_dir_all(&old_dir).unwrap();
        fs::create_dir_all(&plain_sibling).unwrap();
        fs::write(&unrelated_file, "keep me").unwrap();
        fs::create_dir_all(&other_prefix).unwrap();

        clean_stale(&target);

        assert!(!new_dir.exists(), "target.new-* must be removed");
        assert!(!old_dir.exists(), "target.old-* must be removed");
        assert!(plain_sibling.is_dir(), "unrelated sibling directory must survive");
        assert!(unrelated_file.is_file(), "unrelated similar-prefix file must survive");
        assert!(other_prefix.is_dir(), "different-base .new-* must survive");
    }

    #[cfg(unix)]
    fn symlink_at(target: &Path, link: &Path) {
        std::os::unix::fs::symlink(target, link).expect("create symlink");
    }

    #[cfg(windows)]
    fn symlink_at(target: &Path, link: &Path) {
        std::os::windows::fs::symlink_dir(target, link).expect("create symlink");
    }

    #[test]
    fn remove_handles_file_directory_symlink_and_missing_path() {
        let _guard = serial();
        let case = TempCase::new("remove");
        let root = &case.root;

        let file = root.join("plain-file");
        fs::write(&file, "x").unwrap();
        remove(&file).expect("remove file");
        assert!(!file.exists(), "file must be removed");

        let dir = root.join("tree");
        fs::create_dir_all(dir.join("nested")).unwrap();
        fs::write(dir.join("nested/leaf"), "x").unwrap();
        remove(&dir).expect("remove directory");
        assert!(!dir.exists(), "directory tree must be removed");

        let link_target = root.join("link-target");
        fs::create_dir_all(&link_target).unwrap();
        fs::write(link_target.join("keep"), "x").unwrap();
        let link = root.join("a-symlink");
        symlink_at(&link_target, &link);
        remove(&link).expect("remove symlink");
        assert!(fs::symlink_metadata(&link).is_err(), "symlink itself must be gone");
        assert!(link_target.join("keep").is_file(), "symlink target must be untouched");

        remove(&root.join("never-existed")).expect("missing path is not an error");
    }
}
