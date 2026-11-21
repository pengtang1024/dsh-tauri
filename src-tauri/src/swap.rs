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
