//! Where the researcher's outputs land, and how the app reaches them.
//!
//! The backend writes every file a turn produces into one directory per thread
//! (`minime_local/workspace.py:workspace_root`), and until now that was
//! `~/.mini-me/workspaces` **inside the WSL distro** — a place a Windows researcher cannot
//! reach without knowing what `\\wsl.localhost` means. Since ~98% of users are on Windows
//! and none of them are expected to code, files they cannot find are files that do not
//! exist.
//!
//! So the app now chooses that directory instead of letting the backend default, and
//! chooses one on the **Windows** side: `Documents\Mini-Me`. Three things fall out of that
//! single decision — outputs are in Explorer where a scientist expects them, "download
//! everything" becomes a button that opens a folder rather than a zip to build and unpack,
//! and the app can read the plots a turn produced and show them in the chat.
//!
//! The cost is that writes cross WSL's 9p mount. That is genuinely slow for *many small*
//! files — it is why the backend venv is deliberately kept inside the distro — but a
//! turn's outputs are a handful of CSVs, figures and reports, and being able to find them
//! is worth more than the milliseconds.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

/// The variable the backend reads for this (`minime_local/workspace.py:44`).
pub const WORKSPACE_ENV: &str = "MINIME_LOCAL_WORKSPACE";

/// Image kinds worth putting in the transcript.
///
/// `img()` decodes these; SVG is deliberately absent, because gpui renders it through a
/// different element that wants a monochrome-tintable asset rather than a plot.
const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

/// The directory holding one subdirectory per thread.
pub fn root() -> PathBuf {
    if let Some(dir) = std::env::var_os(WORKSPACE_ENV) {
        return PathBuf::from(dir);
    }
    // `Documents`, not `LOCALAPPDATA` like the backend checkout: this is the researcher's
    // own work, the one thing here they will want to open, copy and send to a colleague.
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        return PathBuf::from(home).join("Documents").join("Mini-Me");
    }
    PathBuf::from("Mini-Me")
}

/// Where one conversation's files live.
pub fn thread_dir(thread_id: &str) -> PathBuf {
    root().join(thread_id)
}

/// Every image in `dir`, oldest first.
///
/// Sorted by modification time so a turn's figures appear in the order they were drawn,
/// which for a plotting script is the order the analysis went in. Returns empty for a
/// directory that does not exist yet — the common case, since the backend creates it on
/// first write.
pub fn images(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let extension = path.extension()?.to_str()?.to_ascii_lowercase();
            if !IMAGE_EXTENSIONS.contains(&extension.as_str()) {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found.into_iter().map(|(_, path)| path).collect()
}

/// Open a folder in the platform's file manager.
///
/// The whole of "download everything the agent made": the files are already sitting in the
/// researcher's own Documents, so there is nothing to package — only somewhere to point.
pub fn open(path: &Path) -> Result<()> {
    // Create it first. A researcher who clicks this before a turn has written anything
    // should get an empty folder and learn where things go, not an error.
    std::fs::create_dir_all(path)
        .with_context(|| format!("could not create {}", path.display()))?;

    let mut command = if cfg!(windows) {
        // Through `explorer.exe` directly rather than `cmd /c start`, which would flash a
        // console window on a machine where this app is the only thing the user opened.
        let mut command = std::process::Command::new("explorer.exe");
        command.arg(path);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = std::process::Command::new("open");
        command.arg(path);
        command
    } else {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path);
        command
    };

    // Explorer returns a non-zero exit code even when it succeeds, so the status is not
    // worth checking — only whether the process started at all.
    command
        .spawn()
        .with_context(|| format!("could not open {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_images_are_collected_and_in_the_order_they_were_written() {
        let dir = std::env::temp_dir().join(format!("minime-workspace-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temp dir");

        // A report and a dataset are outputs too, but they are not something the chat can
        // render — only figures belong inline.
        for name in ["informe.md", "papas.csv", "notes.txt"] {
            std::fs::write(dir.join(name), b"x").expect("write");
        }
        for name in ["a_first.png", "b_second.JPG", "c_third.webp"] {
            std::fs::write(dir.join(name), b"x").expect("write");
            // Distinct mtimes, or the sort has nothing to order by.
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let found: Vec<String> = images(&dir)
            .iter()
            .filter_map(|path| path.file_name()?.to_str().map(str::to_string))
            .collect();
        assert_eq!(found, vec!["a_first.png", "b_second.JPG", "c_third.webp"]);

        // A directory that does not exist is the normal state before the first write.
        assert!(images(&dir.join("nothing-here")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_root_is_somewhere_the_researcher_can_find() {
        // Explicit configuration always wins, which is what lets the backend and the app
        // agree on one directory rather than each guessing.
        let previous = std::env::var_os(WORKSPACE_ENV);
        // SAFETY: single-threaded test setup; restored below.
        unsafe { std::env::set_var(WORKSPACE_ENV, "/tmp/somewhere-else") };
        assert_eq!(root(), PathBuf::from("/tmp/somewhere-else"));
        assert_eq!(
            thread_dir("abc-123"),
            PathBuf::from("/tmp/somewhere-else").join("abc-123")
        );
        match previous {
            Some(value) => unsafe { std::env::set_var(WORKSPACE_ENV, value) },
            None => unsafe { std::env::remove_var(WORKSPACE_ENV) },
        }
    }
}
