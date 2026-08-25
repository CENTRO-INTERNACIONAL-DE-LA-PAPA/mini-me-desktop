//! Is there a newer build than this one, and may *this* install take it?
//!
//! Asked for in these terms: *"we cannot ask users to download and install everytime. we need a
//! flow of update."* — so the answer has to be a press inside the app, not a link to a web page,
//! which is the same manual download with one fewer click.
//!
//! This module is the **decision** half: what is published, what is running, and whether the two
//! differ in a direction worth acting on. Fetching the zip and swapping the folder come after, and
//! deliberately not here — everything below is pure enough to test without a network or a Windows
//! machine, which is the §267 lesson applied before rather than after.
//!
//! Two refusals are load-bearing:
//!
//! 1. **A source checkout is never updated.** The person most likely to press this button is the
//!    one who built the app with `cargo build`, whose `target/release/` sits inside a git worktree
//!    with real work in it. Unzipping a release over that would destroy it. `backend.rs` makes the
//!    same argument about the backend checkout — *"a checkout somebody pointed us at may be their
//!    working clone… ownership is what gates that, and it must never be assumed"* — and this is
//!    that argument for the app's own folder.
//! 2. **A build ahead of the release is left alone.** Otherwise a developer running 0.4.0 against a
//!    published 0.3.0 would be offered a downgrade, which is exactly what publishing the stale
//!    `v0.2.3` draft would have done to everyone (§267).

use std::path::{Path, PathBuf};

/// The public API, asked without credentials — deliberately.
///
/// The repository is public and a release asset answers an anonymous `GET`; sending a token would
/// mean an update check that works for whoever built the app and fails for everyone else, and the
/// failure would look like "no update available" rather than like a mistake.
pub const LATEST_URL: &str =
    "https://api.github.com/repos/CENTRO-INTERNACIONAL-DE-LA-PAPA/mini-me-desktop/releases/latest";

/// What the Windows bundle is called, from `release.yml`'s `Compress-Archive` step.
///
/// Matched as a **suffix** rather than reconstructed from the tag: the two are written in different
/// files, and a check that rebuilds the name would fail silently the day either changes. Asking
/// "which asset ends in this?" fails loudly instead, with the names it did find.
pub const ASSET_SUFFIX: &str = "-windows-x64.zip";

/// The three files a packaged bundle carries beside the executable (`scripts/package.sh`).
///
/// All three, not any: `target/release/` could plausibly acquire one of these names, and the cost
/// of a false positive here is a researcher's git worktree overwritten by a zip.
const BUNDLE_MARKERS: [&str; 3] = ["overlay", "scripts", "vendor"];

/// What the executable is called inside a downloaded bundle, per `scripts/package.sh`.
///
/// **Deliberately not `#[cfg]`-split, and that is the whole point.** The first version of this
/// constant was `.exe` on Windows and bare elsewhere — the name depended on the platform doing the
/// *inspecting*. But [`ASSET_SUFFIX`] means the thing inspected is always a Windows bundle, so the
/// name inside is always Windows'. On Windows the two happened to agree, so the mistake would have
/// shipped green; it was found by running the real published zip through `unpack` on Linux, where
/// they disagree. Both names are accepted so a bundle `package.sh` built here can be inspected by
/// the same code that inspects a downloaded one.
///
/// Needed by name only for a *download*. The installed side asks about `current_exe()`, whose name
/// is whatever it is, so a researcher who renamed the executable is unaffected.
const BUNDLED_EXECUTABLES: [&str; 2] = ["mini-me-desktop-app.exe", "mini-me-desktop-app"];

/// A version, compared as three numbers rather than as text.
///
/// The field order is the comparison order, which is what `Ord` derives — and it has to be numeric:
/// `"0.10.0" < "0.9.0"` is true as a string, so a text comparison would tell every install on 0.10
/// that 0.9 was newer and offer it the downgrade forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// `0.3.0` or `v0.3.0`; anything else is `None`.
    ///
    /// Trailing pre-release text (`0.3.0-rc1`) is refused rather than silently truncated to
    /// `0.3.0`: two builds that are not the same build must not compare equal.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let mut next = || parts.next()?.parse::<u64>().ok();
        let (major, minor, patch) = (next()?, next()?, next()?);
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    /// The version this executable was compiled as.
    pub fn running() -> Option<Self> {
        Self::parse(env!("CARGO_PKG_VERSION"))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A published build this app could move to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// The tag as published, for the log line and for naming the download.
    pub tag: String,
    /// Where the zip is. Anonymous `GET`, no redirect handling needed beyond following one.
    pub asset: String,
    /// What the zip should weigh. A short read is the ordinary download failure, and it is the one
    /// that would otherwise be unzipped over a working install.
    pub size: u64,
    /// The release notes, so "what changed" is answerable without opening a browser.
    pub notes: String,
    /// What GitHub says the asset hashes to, as lowercase hex without the `sha256:` prefix.
    ///
    /// `Option` because it is GitHub's field to publish, not ours, and an update must not become
    /// impossible the day they stop sending it — see [`verify`] for what its absence costs.
    pub digest: Option<String>,
}

/// Which checks a download actually passed, so the log can say rather than imply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integrity {
    /// Length and digest both matched what was published.
    Digest,
    /// Length matched; the release published no digest to compare against.
    SizeOnly,
}

/// Where this install stands against the published build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// Running exactly what is published.
    Current,
    /// Something newer exists.
    Behind(Release),
    /// Running something newer than what is published — a developer build. Never offered anything.
    Ahead,
    /// The question could not be answered, and this says why in words a person can act on.
    Unknown(String),
}

/// What kind of install this is, and therefore whether it may be replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layout {
    /// An unzipped bundle. The path is the folder that would be replaced.
    Packaged(PathBuf),
    /// A `cargo build` inside a checkout. Never replaced — see the module note.
    Source,
}

/// Read the answer to `GET /releases/latest`.
///
/// Errors are sentences rather than codes, because they are shown to a researcher and because a
/// message that only says *that* something failed is the mistake this project made three times in
/// one day (§261).
pub fn decode_release(payload: &str) -> Result<Release, String> {
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| format!("unreadable answer: {error}"))?;

    // `/releases/latest` excludes drafts and prereleases by construction, but this decoder is also
    // pointed at `/releases` in tests and could be pointed there for a beta channel later. Checking
    // is two lines; discovering it by shipping a draft to everyone is not.
    if value.get("draft").and_then(serde_json::Value::as_bool) == Some(true) {
        return Err("the latest release is still a draft".to_string());
    }
    if value.get("prerelease").and_then(serde_json::Value::as_bool) == Some(true) {
        return Err("the latest release is a prerelease".to_string());
    }

    let tag = value
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "the answer carried no tag_name".to_string())?;
    let version = Version::parse(tag)
        .ok_or_else(|| format!("{tag} is not a version this app knows how to compare"))?;

    let assets = value
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| format!("{tag} carried no assets"))?;
    let mut names = Vec::new();
    for asset in assets {
        let Some(name) = asset.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if !name.ends_with(ASSET_SUFFIX) {
            names.push(name.to_string());
            continue;
        }
        let url = asset
            .get("browser_download_url")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("{name} carried no download url"))?;
        let size = asset
            .get("size")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| format!("{name} carried no size"))?;
        return Ok(Release {
            version,
            tag: tag.to_string(),
            asset: url.to_string(),
            size,
            notes: value
                .get("body")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            // `sha256:aab3bc…`. Only that algorithm is understood, so anything else is dropped
            // rather than stored under a name that would make `verify` compare the wrong thing.
            digest: asset
                .get("digest")
                .and_then(serde_json::Value::as_str)
                .and_then(|digest| digest.strip_prefix("sha256:"))
                .map(|hex| hex.trim().to_ascii_lowercase()),
        });
    }
    // Names the thing it wanted *and* what was there, so a rename in `release.yml` is one line of
    // log away from being understood rather than a silent "you are up to date".
    Err(format!(
        "{tag} has no {ASSET_SUFFIX} asset (it carries: {})",
        if names.is_empty() {
            "nothing".to_string()
        } else {
            names.join(", ")
        }
    ))
}

/// Compare what is running against what is published.
pub fn standing(running: Option<Version>, latest: &Release) -> Standing {
    let Some(running) = running else {
        return Standing::Unknown(format!(
            "this build reports its version as {}, which cannot be compared",
            env!("CARGO_PKG_VERSION")
        ));
    };
    match running.cmp(&latest.version) {
        std::cmp::Ordering::Less => Standing::Behind(latest.clone()),
        std::cmp::Ordering::Equal => Standing::Current,
        std::cmp::Ordering::Greater => Standing::Ahead,
    }
}

/// Whether the folder holding this executable is a bundle the updater may replace.
pub fn layout(executable: &Path) -> Layout {
    let Some(folder) = executable.parent() else {
        return Layout::Source;
    };
    // The executable too, not only its folder's markers. A staged download of three empty
    // directories would otherwise pass as a bundle, and the swap would leave a folder with no app
    // in it — the one outcome worse than not updating.
    if executable.is_file()
        && BUNDLE_MARKERS
            .iter()
            .all(|marker| folder.join(marker).is_dir())
    {
        Layout::Packaged(folder.to_path_buf())
    } else {
        Layout::Source
    }
}

/// How far taking an update has got.
///
/// A progress variant rather than a bare result because the download is ten megabytes: on a slow
/// connection that is a minute in which a desktop app with no feedback looks like it has hung. The
/// roadmap has carried *"a loading state worth looking at"* as an open item since §177.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetch {
    /// Bytes so far, and the total that was published.
    Progress(u64, u64),
    /// Downloaded, verified and unpacked. The path is the staged bundle.
    Ready(PathBuf, Integrity),
    /// Gave up, with the reason in words.
    Failed(String),
}

/// Where a download is staged: beside the install, not in the system temp folder.
///
/// Same volume as the folder it will replace, which is what lets the swap be a move rather than a
/// copy — and on Windows a move within a volume is the closest thing to atomic available. It is
/// also visible: a researcher who abandons an update can see the folder and delete it.
///
/// Creating it is the *first* thing done, before a byte is downloaded, because "this folder is not
/// writable" is a failure worth having in the first second rather than the sixtieth.
pub fn staging(install: &Path, tag: &str) -> PathBuf {
    let name = format!(".mini-me-update-{}", tag.trim_start_matches('v'));
    install
        .parent()
        .unwrap_or(install)
        .join(name)
}

/// Is this download the thing that was published?
///
/// **This is a corruption check, and calling it anything stronger would be the §252 mistake
/// again.** The integrity guarantee is HTTPS to github.com with a validated certificate; the
/// digest is published by the same authority that publishes the zip, so an attacker able to alter
/// one could alter the other. What it does catch is the ordinary failure: a truncated download, a
/// proxy that returned an error page with a 200, a mixed-up asset. Those are worth catching,
/// because the next step unzips this over a working install.
///
/// The length is checked first and separately, because it is the failure that actually happens and
/// because "10485760 bytes, expected 10793025" is a sentence someone can act on where a hash
/// mismatch is not.
pub fn verify(bytes: &[u8], release: &Release) -> Result<Integrity, String> {
    let got = bytes.len() as u64;
    if got != release.size {
        return Err(format!(
            "the download is {got} bytes and {} was published as {} — a short read, most likely",
            release.tag, release.size
        ));
    }
    let Some(expected) = release.digest.as_deref() else {
        return Ok(Integrity::SizeOnly);
    };
    use sha2::Digest as _;
    let actual = format!("{:x}", sha2::Sha256::digest(bytes));
    if actual == expected {
        Ok(Integrity::Digest)
    } else {
        Err(format!(
            "the download does not match the digest {} published: got {actual}, expected {expected}",
            release.tag
        ))
    }
}

/// Unpack a verified bundle into `into`, and answer with the folder that holds the executable.
///
/// Two things are refused rather than trusted:
///
/// 1. **Any entry that would land outside `into`.** `zip`'s own `enclosed_name` returns `None` for
///    a path containing `..` or a root, and the join is checked again afterwards. We produce this
///    zip ourselves, so this is not a live threat — it is the guard that stays correct if that ever
///    stops being true, and it costs four lines.
/// 2. **A bundle that does not look like one.** What comes out is validated with [`layout`], the
///    same function that decides whether the *installed* folder may be replaced. One definition of
///    "a bundle", used at both ends, so the two cannot drift apart.
pub fn unpack(zip_bytes: &[u8], into: &Path) -> Result<PathBuf, String> {
    let reader = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|error| format!("the download is not a readable zip: {error}"))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("could not read entry {index}: {error}"))?;
        let Some(relative) = entry.enclosed_name() else {
            return Err(format!(
                "the download contains an unsafe path ({}) and was not unpacked",
                entry.name()
            ));
        };
        let target = into.join(&relative);
        if !target.starts_with(into) {
            return Err(format!(
                "the download would write outside the staging folder ({})",
                relative.display()
            ));
        }
        if entry.is_dir() {
            std::fs::create_dir_all(&target)
                .map_err(|error| format!("could not make {}: {error}", target.display()))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("could not make {}: {error}", parent.display()))?;
        }
        let mut file = std::fs::File::create(&target)
            .map_err(|error| format!("could not write {}: {error}", target.display()))?;
        std::io::copy(&mut entry, &mut file)
            .map_err(|error| format!("could not write {}: {error}", target.display()))?;
    }
    bundle_root(into)
}

/// The folder inside `unpacked` that is a bundle, or a sentence saying why none of them is.
///
/// `package.sh` writes `dist/mini-me-desktop/` and `Compress-Archive` keeps that folder, so the
/// bundle is one level down — but this looks at `unpacked` itself first, so a flattened zip would
/// still work rather than fail on a layout detail.
pub fn bundle_root(unpacked: &Path) -> Result<PathBuf, String> {
    if let Some(folder) = bundle_at(unpacked) {
        return Ok(folder);
    }
    let entries = std::fs::read_dir(unpacked)
        .map_err(|error| format!("could not read {}: {error}", unpacked.display()))?;
    let mut seen = Vec::new();
    for entry in entries.flatten() {
        let candidate = entry.path();
        if !candidate.is_dir() {
            continue;
        }
        if let Some(folder) = bundle_at(&candidate) {
            return Ok(folder);
        }
        seen.push(entry.file_name().to_string_lossy().into_owned());
    }
    Err(format!(
        "the download does not contain a bundle: no {} beside {} (it holds: {})",
        BUNDLED_EXECUTABLES.join(" or "),
        BUNDLE_MARKERS.join(", "),
        if seen.is_empty() {
            "nothing".to_string()
        } else {
            seen.join(", ")
        }
    ))
}

/// Is this folder itself a bundle, under either name the executable can carry?
fn bundle_at(folder: &Path) -> Option<PathBuf> {
    BUNDLED_EXECUTABLES
        .iter()
        .map(|name| folder.join(name))
        .find_map(|app| match layout(&app) {
            Layout::Packaged(root) => Some(root),
            Layout::Source => None,
        })
}

/// Everything the helper needs to replace this install after this process exits.
///
/// A struct rather than six arguments because the script is generated from it and asserted against
/// it, and because getting `install` and `staged` the wrong way round would delete the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Swap {
    /// The process that must exit first — this one.
    pub pid: u32,
    /// The folder being replaced.
    pub install: PathBuf,
    /// The unpacked bundle that replaces it.
    pub staged: PathBuf,
    /// Where the old folder is moved to, so a failure can put it back.
    pub retired: PathBuf,
    /// The staging folder to clean up, which is `staged`'s parent.
    pub staging: PathBuf,
    /// What to launch when the folders are in place.
    pub launch: PathBuf,
    /// Where the helper writes what it did, since by then nothing else is watching.
    pub log: PathBuf,
    /// The working directory the helper runs in, which must be **outside** [`Self::install`].
    ///
    /// A spawned process inherits its parent's working directory, and when a researcher
    /// double-clicks the executable that directory *is* the install folder. Windows refuses to
    /// rename a directory that is a running process's current directory, so the helper would have
    /// held open the very folder it was about to move and failed at step 2 with a sharing
    /// violation — on the first real swap, having passed every test here, because a script's text
    /// says nothing about the process that runs it.
    pub working: PathBuf,
}

impl Swap {
    /// A short label for the log line written before the helper starts.
    ///
    /// Derived from the staging folder rather than carried separately: one fewer field that can be
    /// set to something the rest of the plan disagrees with.
    pub fn tag_hint(&self) -> String {
        self.staging
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "update".to_string())
    }

    /// Work out the whole plan from the two folders, so no caller can pair them wrongly.
    pub fn plan(pid: u32, install: &Path, staged: &Path, tag: &str) -> Self {
        let parent = install.parent().unwrap_or(install).to_path_buf();
        let version = tag.trim_start_matches('v');
        Self {
            pid,
            install: install.to_path_buf(),
            staged: staged.to_path_buf(),
            retired: parent.join(format!(".mini-me-previous-{version}")),
            staging: staged.parent().unwrap_or(staged).to_path_buf(),
            launch: install.join(BUNDLED_EXECUTABLES[0]),
            log: std::env::temp_dir().join("mini-me-desktop-update.log"),
            // The temp folder: guaranteed to exist, guaranteed not to be the thing being moved.
            working: std::env::temp_dir(),
        }
    }
}

/// A path as a PowerShell single-quoted literal.
///
/// Single quotes because PowerShell does no expansion inside them: a folder called `$env` or
/// `` `n `` is data, not code. Doubling is the only escape that applies.
fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

/// How long the helper waits for this process to let go of its files.
const EXIT_GRACE_SECONDS: u32 = 60;

/// The script that replaces the install once this process is gone.
///
/// **Windows cannot overwrite a running `.exe`**, which is the whole reason a helper exists rather
/// than a few lines of Rust: something has to outlive the app to move its folder.
///
/// The order is the safety property, and every step is a rename within one parent — the same
/// volume, so each is a metadata operation rather than a ten-megabyte copy:
///
/// 1. **Wait, and give up rather than force it.** If the app is still running after
///    [`EXIT_GRACE_SECONDS`] nothing is touched at all. An abandoned update is a nuisance; a
///    half-moved one is a researcher with no working app.
/// 2. **Retire, do not delete.** The old folder is renamed aside, never removed, so step 3 has
///    something to put back.
/// 3. **Move the new one in. If that fails, put the old one back.** The rollback is the reason
///    step 2 is a rename.
/// 4. **Launch, then clean up.** In that order: a failure to delete a leftover folder must not
///    stop the app from starting. The new app is started *in* the install folder, because that is
///    where a double-click starts it — the helper's own working directory is elsewhere by
///    necessity, and inheriting that would make an updated app subtly unlike a fresh one.
///
/// **It never opens the log itself.** `begin_swap` hands the child the log as its stdout, so `Note`
/// writes to the output stream and the bytes land there — one writer, one handle.
///
/// The first version used `Out-File -Append` on that same path, and the child could not open a file
/// it already held open as stdout: `Out-File` asks for a share mode the existing handle
/// contradicts. The very first `Note` failed, `$ErrorActionPreference = 'Stop'` made it terminating,
/// and the helper died at line one having done nothing. The only reason it was ever diagnosable is
/// that PowerShell put the error on *stderr* — the same file by a different handle (§274).
pub fn swap_script(plan: &Swap) -> String {
    let (install, staged, retired, staging, launch) = (
        quote(&plan.install),
        quote(&plan.staged),
        quote(&plan.retired),
        quote(&plan.staging),
        quote(&plan.launch),
    );
    let pid = plan.pid;
    let grace = EXIT_GRACE_SECONDS;
    format!(
        "$ErrorActionPreference = 'Stop'; \
         function Note($m) {{ Write-Output \"$(Get-Date -Format o) $m\" }}; \
         Note 'waiting for mini-me-desktop-app (pid {pid}) to exit'; \
         $left = {grace}; \
         while ($left -gt 0 -and (Get-Process -Id {pid} -ErrorAction SilentlyContinue)) {{ \
         Start-Sleep -Seconds 1; $left-- }}; \
         if (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ \
         Note 'it is still running after {grace}s, so nothing was changed'; exit 1 }}; \
         Start-Sleep -Milliseconds 750; \
         try {{ \
         if (Test-Path -LiteralPath {retired}) {{ Remove-Item -LiteralPath {retired} -Recurse -Force }}; \
         Move-Item -LiteralPath {install} -Destination {retired} -Force; \
         Note 'retired the old folder' \
         }} catch {{ Note \"could not move the old folder aside: $_\"; exit 1 }}; \
         try {{ \
         Move-Item -LiteralPath {staged} -Destination {install} -Force; \
         Note 'the new build is in place' \
         }} catch {{ \
         Note \"could not move the new build in, putting the old one back: $_\"; \
         Move-Item -LiteralPath {retired} -Destination {install} -Force; \
         Note 'the old build is back'; exit 1 }}; \
         try {{ Start-Process -FilePath {launch} -WorkingDirectory {install}; Note 'relaunched' }} \
         catch {{ Note \"the new build is in place but did not start: $_\" }}; \
         Remove-Item -LiteralPath {retired} -Recurse -Force -ErrorAction SilentlyContinue; \
         Remove-Item -LiteralPath {staging} -Recurse -Force -ErrorAction SilentlyContinue; \
         Note 'done'"
    )
}

/// The script as PowerShell wants it for `-EncodedCommand`: UTF-16LE, then base64.
///
/// **This exists because `-Command` did not work and could not be debugged from here.** The spawn
/// succeeded, PowerShell started, and nothing was written — not even the log's first line, because
/// PowerShell parses the whole `-Command` string *before* running any of it, so one bad character
/// anywhere silences everything. The script contains double quotes; `notify.rs`, which works, does
/// not. Rust escapes them for the command line, PowerShell re-parses them, and somewhere in that
/// handoff the script stopped being valid.
///
/// Rather than guess which character it was — there is no PowerShell on the machine this is written
/// on, so the guess could not be checked — `-EncodedCommand` removes the whole class. The argument
/// is opaque base64: nothing between here and the parser can reinterpret a quote, a brace or a
/// semicolon (§270).
pub fn encoded_script(script: &str) -> String {
    use base64::Engine as _;
    let utf16: Vec<u8> = script
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    base64::engine::general_purpose::STANDARD.encode(utf16)
}

/// The whole command, ready to spawn.
pub fn swap_command(plan: &Swap) -> Vec<String> {
    vec![
        "powershell".to_string(),
        "-NoProfile".to_string(),
        // Nothing here reads from a console, and a prompt nobody can answer would hang the swap.
        "-NonInteractive".to_string(),
        "-WindowStyle".to_string(),
        "Hidden".to_string(),
        "-EncodedCommand".to_string(),
        encoded_script(&swap_script(plan)),
    ]
}

/// Start the helper, which outlives this process on purpose.
///
/// Returns `Ok` when the helper is *running*, which is not the same as the swap having succeeded —
/// nothing here can know that, because the caller's next act is to exit. What the helper did is in
/// `%TEMP%\mini-me-desktop-update.log`.
pub fn begin_swap(plan: &Swap) -> Result<(), String> {
    let argv = swap_command(plan);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        // **The child's own output goes to the update log.** Twice now a press has produced
        // nothing at all — no restart, no log, no error — because the helper died before its first
        // statement and, being detached, had no console for the message to land on. Handing it the
        // log as stdout and stderr means the next failure of that shape writes *something*, even
        // if the script never runs. A failure that leaves no trace costs a round trip to a person
        // on another machine, which is the most expensive kind there is (§270).
        let mut record = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&plan.log)
            .map_err(|error| format!("could not open {}: {error}", plan.log.display()))?;

        // **A line of our own, before the child exists.** An empty log is ambiguous in exactly the
        // wrong way: "the helper never started" and "the helper started and said nothing" look
        // identical, and they need opposite fixes. This one line makes the file say which — it is
        // written by this process, so its presence proves the spawn was reached and its
        // *loneliness* proves the child contributed nothing (§272).
        use std::io::Write as _;
        let _ = writeln!(
            record,
            "--- {} about to start the helper: powershell {} args, pid {} to wait for, install {}",
            plan.tag_hint(),
            argv.len() - 1,
            plan.pid,
            plan.install.display()
        );
        let _ = record.flush();

        let errors = record
            .try_clone()
            .map_err(|error| format!("could not share the update log: {error}"))?;
        // **`CREATE_NO_WINDOW`, and not `DETACHED_PROCESS`.** These two and `CREATE_NEW_CONSOLE`
        // are mutually exclusive — passing more than one makes `CreateProcess` fail with
        // `ERROR_INVALID_PARAMETER` — so exactly one has to be chosen, and the first two choices
        // were both wrong:
        //
        // 1. `CREATE_NO_WINDOW | DETACHED_PROCESS` failed at the spawn, rejected outright (§269).
        // 2. `DETACHED_PROCESS` alone spawned successfully and PowerShell died before its first
        //    statement, leaving a log this process had created and the child never wrote to. It
        //    gives the child **no console at all**, and `powershell.exe` is a console application:
        //    with stderr redirected it emits CLIXML progress records before running anything, and
        //    not one of those arrived either (§273).
        //
        // `CREATE_NO_WINDOW` gives the child its own console and hides it, which is what
        // `notify.rs` has used to run PowerShell successfully in this app since §265 — the working
        // example that was sitting here the whole time. Nothing kills a child when its parent
        // exits on Windows, so the helper still outlives the app without being detached.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new(&argv[0])
            .args(&argv[1..])
            // Outside the folder being replaced — see `Swap::working`.
            .current_dir(&plan.working)
            .stdout(record)
            .stderr(errors)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("could not start the updater: {error}"))
    }
    #[cfg(not(windows))]
    {
        // The bundle is Windows-only, so there is nothing to swap anywhere else. Logged rather
        // than silently ignored so a Linux run says why the button did nothing.
        tracing::info!(
            steps = argv.len(),
            "an update swap was requested on a platform that has no bundle"
        );
        Err("taking an update is only implemented on Windows".to_string())
    }
}

/// One line for the About page, whatever the answer.
///
/// Every branch says something, including the failures: a check that goes quiet when it cannot
/// reach GitHub is indistinguishable from one that found nothing, and the researcher would read
/// silence as "up to date".
pub fn describe(standing: &Standing, layout: &Layout) -> String {
    match (standing, layout) {
        (Standing::Behind(release), Layout::Packaged(_)) => {
            format!("{} is available — you have {}", release.tag, running_text())
        }
        (Standing::Behind(release), Layout::Source) => format!(
            "{} is published, but this build came from source — update it with git",
            release.tag
        ),
        (Standing::Current, _) => format!("{} — the newest published build", running_text()),
        (Standing::Ahead, _) => format!("{} — newer than anything published", running_text()),
        (Standing::Unknown(reason), _) => format!("could not check for updates: {reason}"),
    }
}

fn running_text() -> String {
    Version::running().map_or_else(
        || env!("CARGO_PKG_VERSION").to_string(),
        |version| version.to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real answer from the real endpoint, saved 2026-08-21 the day `v0.3.0` was published.
    const LATEST: &str = include_str!("../tests/fixtures/github-latest-release.json");

    #[test]
    fn a_version_is_three_numbers_with_or_without_a_v() {
        assert_eq!(
            Version::parse("0.3.0"),
            Some(Version {
                major: 0,
                minor: 3,
                patch: 0
            })
        );
        assert_eq!(Version::parse("v0.3.0"), Version::parse("0.3.0"));
        assert_eq!(Version::parse(" v1.20.300 "), Version::parse("1.20.300"));
        // Not versions this app can compare, so it must say so rather than guess.
        assert_eq!(Version::parse("0.3"), None);
        assert_eq!(Version::parse("0.3.0.1"), None);
        assert_eq!(Version::parse("0.3.0-rc1"), None);
        assert_eq!(Version::parse("latest"), None);
        assert_eq!(Version::parse(""), None);
    }

    /// The trap this type exists for.
    ///
    /// `"0.10.0" < "0.9.0"` is **true** as text. An updater comparing tags as strings would tell
    /// every install on 0.10 that 0.9 was newer, offer the downgrade, and go on offering it after
    /// it was taken — a loop from one missing `parse`.
    #[test]
    fn ten_is_newer_than_nine_which_text_comparison_gets_wrong() {
        let ten = Version::parse("0.10.0").expect("a version");
        let nine = Version::parse("0.9.0").expect("a version");
        assert!(ten > nine, "0.10.0 must be newer than 0.9.0");
        assert!("0.10.0" < "0.9.0", "…which comparing the text would get backwards");

        assert!(Version::parse("1.0.0").unwrap() > Version::parse("0.99.99").unwrap());
        assert!(Version::parse("0.3.1").unwrap() > Version::parse("0.3.0").unwrap());
    }

    #[test]
    fn the_published_release_is_read_off_the_real_answer() {
        let release = decode_release(LATEST).expect("the real payload decodes");
        assert_eq!(release.tag, "v0.3.0");
        assert_eq!(release.version, Version::parse("0.3.0").unwrap());
        assert_eq!(release.size, 10_793_025);
        assert!(
            release.asset.ends_with("mini-me-desktop-v0.3.0-windows-x64.zip"),
            "{}",
            release.asset
        );
        assert!(release.asset.starts_with("https://"), "{}", release.asset);
        // The notes are carried so "what changed" is answerable without a browser.
        assert!(release.notes.contains("AutoDiscovery"), "the notes came through");
    }

    /// The size is what a truncated download is caught by, so it must be the asset's own number and
    /// not something plausible-looking read off the wrong field.
    #[test]
    fn the_size_is_the_assets_own_and_matches_what_was_published() {
        let release = decode_release(LATEST).expect("decodes");
        let value: serde_json::Value = serde_json::from_str(LATEST).expect("json");
        let published = value["assets"][0]["size"].as_u64().expect("a size");
        assert_eq!(release.size, published);
        assert!(release.size > 1_000_000, "a bundle is megabytes, not bytes");
    }

    #[test]
    fn a_draft_or_a_prerelease_is_not_offered() {
        let mut value: serde_json::Value = serde_json::from_str(LATEST).expect("json");
        value["draft"] = serde_json::Value::Bool(true);
        let error = decode_release(&value.to_string()).expect_err("a draft is not a release");
        assert!(error.contains("draft"), "{error}");

        let mut value: serde_json::Value = serde_json::from_str(LATEST).expect("json");
        value["prerelease"] = serde_json::Value::Bool(true);
        let error = decode_release(&value.to_string()).expect_err("a prerelease is not offered");
        assert!(error.contains("prerelease"), "{error}");
    }

    /// A renamed asset must fail loudly. The alternative — treating "no zip I recognise" as "no
    /// update" — is how a rename in `release.yml` would quietly strand every install.
    #[test]
    fn a_missing_asset_names_what_it_wanted_and_what_was_there() {
        let mut value: serde_json::Value = serde_json::from_str(LATEST).expect("json");
        value["assets"][0]["name"] = serde_json::Value::String("mini-me-linux.tar.gz".into());
        let error = decode_release(&value.to_string()).expect_err("no windows asset");
        assert!(error.contains(ASSET_SUFFIX), "it must say what it wanted: {error}");
        assert!(
            error.contains("mini-me-linux.tar.gz"),
            "and what it found instead: {error}"
        );
    }

    #[test]
    fn an_unreadable_answer_is_a_sentence_not_a_silence() {
        let error = decode_release("not json at all").expect_err("unreadable");
        assert!(error.contains("unreadable answer"), "{error}");
        let error = decode_release("{}").expect_err("no tag");
        assert!(error.contains("tag_name"), "{error}");
    }

    fn release_at(tag: &str) -> Release {
        Release {
            version: Version::parse(tag).expect("a version"),
            tag: tag.to_string(),
            asset: "https://example.invalid/x.zip".to_string(),
            size: 1,
            notes: String::new(),
            digest: None,
        }
    }

    #[test]
    fn a_newer_release_is_offered_and_an_older_one_is_not() {
        let running = Version::parse("0.3.0");
        assert_eq!(
            standing(running, &release_at("0.3.1")),
            Standing::Behind(release_at("0.3.1"))
        );
        assert_eq!(standing(running, &release_at("0.3.0")), Standing::Current);
        // The §267 failure, in the other direction: a developer on 0.4.0 must never be handed 0.3.0.
        assert_eq!(standing(running, &release_at("0.3.0")), Standing::Current);
        assert_eq!(standing(Version::parse("0.4.0"), &release_at("0.3.0")), Standing::Ahead);
        assert_eq!(standing(Version::parse("0.10.0"), &release_at("0.9.9")), Standing::Ahead);
    }

    #[test]
    fn a_build_that_cannot_name_its_version_is_never_updated() {
        match standing(None, &release_at("9.9.9")) {
            Standing::Unknown(reason) => assert!(!reason.is_empty(), "it must say why"),
            other => panic!("an uncomparable build must not be offered anything: {other:?}"),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "mini-me-update-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("scratch");
        base
    }

    /// The refusal that protects a developer's checkout.
    ///
    /// `cargo build` leaves the executable in `target/release/` with none of a bundle's folders
    /// beside it. Unzipping a release over that directory would replace a git worktree with a
    /// release build — the app's own version of the mistake `resolve_project_dir` refuses to make
    /// with the backend checkout.
    #[test]
    fn a_source_build_is_not_a_bundle_and_is_never_replaced() {
        let base = scratch("source");
        let target = base.join("target/release");
        std::fs::create_dir_all(&target).expect("target");
        std::fs::write(target.join("mini-me-desktop-app"), b"a build").expect("exe");
        assert_eq!(layout(&target.join("mini-me-desktop-app")), Layout::Source);

        // Two of the three markers is still not a bundle: all three, or none of it.
        for marker in ["overlay", "scripts"] {
            std::fs::create_dir_all(target.join(marker)).expect("marker");
        }
        assert_eq!(
            layout(&target.join("mini-me-desktop-app")),
            Layout::Source,
            "a partial match must not be taken for a bundle"
        );

        std::fs::create_dir_all(target.join("vendor")).expect("marker");
        assert_eq!(
            layout(&target.join("mini-me-desktop-app")),
            Layout::Packaged(target.clone()),
            "all three markers is what an unzipped bundle looks like"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    /// The layout `scripts/package.sh` actually writes, read off the script rather than remembered.
    #[test]
    fn the_markers_are_the_folders_the_packager_puts_there() {
        let packager = include_str!("../../../scripts/package.sh");
        for marker in BUNDLE_MARKERS {
            assert!(
                packager.contains(marker),
                "{marker} is not something package.sh puts in the bundle"
            );
        }
        assert!(
            packager.contains("for dir in overlay scripts"),
            "package.sh no longer copies overlay/ and scripts/ the way this check assumes"
        );
        assert!(
            packager.contains("$OUT/vendor"),
            "package.sh no longer writes vendor/ into the bundle"
        );
        // The name the executable carries inside a bundle, which is the thing a `#[cfg]` split got
        // wrong: it made the name depend on the platform *inspecting* the zip rather than on the
        // zip. The real published bundle disagreed with every synthetic one here, and only running
        // it through `unpack` showed that.
        for name in BUNDLED_EXECUTABLES {
            assert!(
                packager.contains(name),
                "package.sh does not put {name} in the bundle, so unpack is looking for the \
                 wrong file"
            );
        }
    }

    /// The asset name is written in `release.yml` and matched here; the two must not drift.
    #[test]
    fn the_asset_suffix_is_the_one_the_workflow_builds() {
        let workflow = include_str!("../../../.github/workflows/release.yml");
        assert!(
            workflow.contains(&format!("mini-me-desktop-$tag{ASSET_SUFFIX}")),
            "release.yml no longer names the bundle mini-me-desktop-<tag>{ASSET_SUFFIX}"
        );
    }

    /// Every branch says something. A check that goes quiet on failure reads as "up to date".
    #[test]
    fn every_answer_has_a_sentence_including_the_failures() {
        let packaged = Layout::Packaged(PathBuf::from("/x"));
        let behind = describe(&Standing::Behind(release_at("9.9.9")), &packaged);
        assert!(behind.contains("9.9.9") && behind.contains("available"), "{behind}");

        // A source build is told the truth: there is a newer one, and this is not the way to it.
        let source = describe(&Standing::Behind(release_at("9.9.9")), &Layout::Source);
        assert!(source.contains("git"), "{source}");

        assert!(describe(&Standing::Current, &packaged).contains("newest"));
        assert!(describe(&Standing::Ahead, &packaged).contains("newer than anything"));

        let unknown = describe(&Standing::Unknown("no network".into()), &packaged);
        assert!(unknown.contains("no network"), "the reason must survive: {unknown}");
        assert!(unknown.contains("could not check"), "{unknown}");
    }

    /// The digest comes off the real payload, and it is the one `v0.3.0` really hashes to.
    ///
    /// Checked by hand against the published zip on the day it was cut:
    ///
    /// ```text
    /// published: aab3bc3838f71b5c2871b4a10a5b394f56461ed898f9c7d5a779980b71447ffb
    /// actual   : aab3bc3838f71b5c2871b4a10a5b394f56461ed898f9c7d5a779980b71447ffb
    /// ```
    #[test]
    fn the_digest_is_read_off_the_asset_without_its_prefix() {
        let release = decode_release(LATEST).expect("decodes");
        assert_eq!(
            release.digest.as_deref(),
            Some("aab3bc3838f71b5c2871b4a10a5b394f56461ed898f9c7d5a779980b71447ffb")
        );
    }

    /// An algorithm this code cannot compute must not be stored as though it could.
    #[test]
    fn a_digest_in_another_algorithm_is_dropped_rather_than_kept() {
        let mut value: serde_json::Value = serde_json::from_str(LATEST).expect("json");
        value["assets"][0]["digest"] = serde_json::Value::String("sha512:00ff".into());
        let release = decode_release(&value.to_string()).expect("decodes");
        assert_eq!(
            release.digest, None,
            "keeping a sha512 as if it were a sha256 would make verify() compare the wrong thing"
        );
    }

    fn release_for(bytes: &[u8], digest: Option<&str>) -> Release {
        Release {
            version: Version::parse("9.9.9").expect("a version"),
            tag: "v9.9.9".to_string(),
            asset: "https://example.invalid/x.zip".to_string(),
            size: bytes.len() as u64,
            notes: String::new(),
            digest: digest.map(str::to_string),
        }
    }

    #[test]
    fn a_download_of_the_right_length_and_digest_passes() {
        let bytes = b"a bundle, notionally".to_vec();
        use sha2::Digest as _;
        let hex = format!("{:x}", sha2::Sha256::digest(&bytes));
        assert_eq!(
            verify(&bytes, &release_for(&bytes, Some(&hex))),
            Ok(Integrity::Digest)
        );
        // Without a published digest the length is all there is, and the answer says so rather
        // than implying more was checked than was.
        assert_eq!(
            verify(&bytes, &release_for(&bytes, None)),
            Ok(Integrity::SizeOnly)
        );
    }

    /// The failure that actually happens, and the message a person can act on.
    #[test]
    fn a_short_read_is_refused_and_both_numbers_are_named() {
        let published = release_for(b"the whole bundle", None);
        let error = verify(b"the whole", &published).expect_err("a short read");
        assert!(error.contains("9 bytes"), "the length it got: {error}");
        assert!(error.contains("16"), "and the length published: {error}");
    }

    #[test]
    fn a_digest_that_does_not_match_is_refused() {
        let bytes = b"a bundle".to_vec();
        let wrong = "0".repeat(64);
        let error = verify(&bytes, &release_for(&bytes, Some(&wrong))).expect_err("mismatch");
        assert!(error.contains(&wrong), "it must name what was expected: {error}");
        assert!(error.contains("does not match"), "{error}");
    }

    /// Build the bundle `package.sh` produces, as a zip, in memory.
    /// Built with the **Windows** executable name, because `-windows-x64.zip` is the only asset
    /// the updater fetches. A synthetic bundle carrying this machine's name instead is what let
    /// `unpack` pass every test here and fail on the real zip.
    fn bundle_zip(root: &str, with_executable: bool) -> Vec<u8> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            for marker in BUNDLE_MARKERS {
                writer
                    .start_file(format!("{root}/{marker}/keep.txt"), options)
                    .expect("entry");
                std::io::Write::write_all(&mut writer, b"x").expect("write");
            }
            if with_executable {
                writer
                    .start_file(format!("{root}/mini-me-desktop-app.exe"), options)
                    .expect("entry");
                std::io::Write::write_all(&mut writer, b"an app").expect("write");
            }
            writer.finish().expect("finish");
        }
        buffer.into_inner()
    }

    #[test]
    fn a_bundle_unpacks_and_its_root_is_found_one_level_down() {
        let base = scratch("unpack");
        let zip_bytes = bundle_zip("mini-me-desktop", true);
        let root = unpack(&zip_bytes, &base).expect("unpacks");
        assert_eq!(root, base.join("mini-me-desktop"));
        assert!(root.join("mini-me-desktop-app.exe").is_file(), "the app came out");
        for marker in BUNDLE_MARKERS {
            assert!(root.join(marker).is_dir(), "{marker} came out");
        }
        // And the same function that guards the *installed* folder agrees this is a bundle.
        assert_eq!(
            layout(&root.join("mini-me-desktop-app.exe")),
            Layout::Packaged(root.clone())
        );
        std::fs::remove_dir_all(&base).ok();
    }

    /// Three folders and no app is not something to swap in — it is the one outcome worse than
    /// not updating, because it leaves a folder with nothing to launch.
    #[test]
    fn a_download_with_no_executable_is_not_a_bundle() {
        let base = scratch("no-exe");
        let zip_bytes = bundle_zip("mini-me-desktop", false);
        let error = unpack(&zip_bytes, &base).expect_err("not a bundle");
        assert!(
            error.contains("mini-me-desktop-app.exe"),
            "it must say what was missing: {error}"
        );
        assert!(error.contains("mini-me-desktop"), "and what it looked in: {error}");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn something_that_is_not_a_zip_is_refused_as_one() {
        let base = scratch("not-zip");
        let error = unpack(b"<html>404 not found</html>", &base).expect_err("not a zip");
        assert!(error.contains("not a readable zip"), "{error}");
        std::fs::remove_dir_all(&base).ok();
    }

    /// Nothing may land outside the staging folder.
    ///
    /// We produce this zip ourselves, so this is not a live threat — it is the guard that stays
    /// correct if that stops being true. The entry is written with the traversal in its name, which
    /// is why the archive is built by hand here rather than with `start_file`.
    #[test]
    fn an_entry_that_climbs_out_of_the_staging_folder_is_refused() {
        let base = scratch("traversal");
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            writer
                .add_directory("../escaped", options)
                .or_else(|_| writer.start_file("../escaped.txt", options))
                .expect("an entry naming a parent");
            let _ = std::io::Write::write_all(&mut writer, b"escaped");
            writer.finish().expect("finish");
        }
        let zip_bytes = buffer.into_inner();
        match unpack(&zip_bytes, &base) {
            Err(error) => assert!(
                error.contains("unsafe path")
                    || error.contains("outside the staging folder")
                    || error.contains("does not contain a bundle"),
                "a traversal must be refused, not written: {error}"
            ),
            Ok(root) => panic!("a traversal unpacked to {}", root.display()),
        }
        // Whatever happened, nothing was written beside the staging folder.
        assert!(
            !base.parent().expect("a parent").join("escaped.txt").exists(),
            "an entry escaped the staging folder"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    /// Beside the install, never inside it.
    ///
    /// Inside would mean the folder being replaced contains the replacement, so the swap would
    /// have to delete its own source. Same parent also means the same volume, which is what makes
    /// the final move a move.
    #[test]
    fn a_download_is_staged_beside_the_install_not_within_it() {
        let install = PathBuf::from("/opt/apps/mini-me-desktop");
        let staged = staging(&install, "v0.4.0");
        assert_eq!(staged.parent(), install.parent(), "the same volume, and the same parent");
        assert!(!staged.starts_with(&install), "{}", staged.display());
        assert!(
            staged.file_name().expect("a name").to_string_lossy().contains("0.4.0"),
            "the tag belongs in the name so two attempts cannot collide: {}",
            staged.display()
        );
        // The `v` is dropped so the folder does not read as a git tag sitting in someone's
        // Documents; and a bare version is accepted too, since the tag is GitHub's to spell.
        assert_eq!(staging(&install, "0.4.0"), staged);
    }

    /// **Forward slashes deliberately.** `Path` accepts them on Windows too, and a backslash is not
    /// a separator on Linux — so a plan built from `C:\Users\x\app` here has *one* component and
    /// `parent()` is `""`, which makes every assertion below pass while measuring nothing. That is
    /// §267's trap wearing a test's clothes, and it caught this file on the first run.
    fn a_plan() -> Swap {
        Swap::plan(
            4242,
            Path::new("C:/Users/LENOVO/Apps/mini-me-desktop"),
            Path::new("C:/Users/LENOVO/Apps/.mini-me-update-0.3.1/mini-me-desktop"),
            "v0.3.1",
        )
    }

    /// The plan is derived, not passed in, so no caller can pair the folders the wrong way round —
    /// which in this script would mean deleting the app instead of replacing it.
    #[test]
    fn the_plan_puts_every_folder_under_one_parent() {
        let plan = a_plan();
        let parent = Path::new("C:/Users/LENOVO/Apps");
        for folder in [&plan.install, &plan.retired, &plan.staging] {
            assert_eq!(folder.parent(), Some(parent), "{}", folder.display());
        }
        assert_ne!(plan.retired, plan.install, "the old folder must move somewhere else");
        assert!(plan.staged.starts_with(&plan.staging));
        assert_eq!(plan.launch, plan.install.join("mini-me-desktop-app.exe"));
        assert!(plan.launch.starts_with(&plan.install), "it launches the new build, not the old");
    }

    /// An updated app should be indistinguishable from a freshly double-clicked one.
    ///
    /// The helper runs outside the install folder by necessity — it cannot rename a directory it
    /// is standing in. Without `-WorkingDirectory` the app it launches would inherit *that*, so an
    /// updated install would run with a different working directory from a fresh one. Nothing
    /// currently depends on it, which is exactly why this is worth pinning: "it only misbehaves
    /// after an update" is a miserable thing to chase a year from now.
    #[test]
    fn the_relaunched_app_starts_where_a_double_click_would() {
        let plan = a_plan();
        let script = swap_script(&plan);
        assert!(
            script.contains(&format!(
                "Start-Process -FilePath {} -WorkingDirectory {}",
                quote(&plan.launch),
                quote(&plan.install)
            )),
            "the new build must start in the install folder, not the helper's: {script}"
        );
        // And not in the helper's own directory, which is the mistake this prevents.
        assert!(
            !script.contains(&format!("-WorkingDirectory {}", quote(&plan.working))),
            "the relaunched app must not inherit the helper's working directory"
        );
    }

    /// One console flag, never two.
    ///
    /// Windows treats `CREATE_NEW_CONSOLE`, `CREATE_NO_WINDOW` and `DETACHED_PROCESS` as **mutually
    /// exclusive**: passing more than one makes `CreateProcess` fail with `ERROR_INVALID_PARAMETER`.
    /// The first version OR'd two together — `CREATE_NO_WINDOW` copied from `notify.rs`, where it is
    /// correct on its own — and every press failed at the spawn. There was no log to diagnose it
    /// from, because writing the log is the helper's job and the helper never started.
    ///
    /// The spawn is `#[cfg(windows)]` and unreachable from here, so this reads the flags off the
    /// source. A narrow assertion on the one expression that matters, not a search of the file.
    #[test]
    fn the_helper_is_spawned_with_one_console_flag() {
        let call = include_str!("update.rs")
            .split("creation_flags(")
            .nth(1)
            .expect("the helper is spawned with creation flags")
            .split(')')
            .next()
            .expect("a flag expression");
        // **No `|` at all**, which is the invariant however a flag is spelled. Checking for the
        // three names alone was not enough: putting the bug back as `0x0800_0000 |
        // DETACHED_PROCESS` left this test green, and a guard that passes when the defect is
        // restored is not a guard.
        assert!(
            !call.contains('|'),
            "these flags are mutually exclusive on Windows, so ORing any two makes CreateProcess \
             fail with ERROR_INVALID_PARAMETER: got `{call}`"
        );
        assert_eq!(
            call.trim(),
            "CREATE_NO_WINDOW",
            "the flag `notify.rs` uses to run PowerShell in this app, and the only one proven to \
             let it start; DETACHED_PROCESS gives the child no console and it died silently (§273)"
        );
    }

    /// The helper must not be standing in the folder it is about to move.
    ///
    /// A spawned process inherits the parent's working directory, and double-clicking the
    /// executable makes that the install folder. Windows will not rename a directory that is a
    /// running process's current directory, so the helper would have held open the folder it was
    /// replacing and failed at the first move — after passing every other test in this file,
    /// because the text of a script says nothing about the process that runs it.
    #[test]
    fn the_helper_does_not_stand_in_the_folder_it_replaces() {
        let plan = a_plan();
        assert!(
            !plan.working.starts_with(&plan.install),
            "the helper would hold open the folder it is moving: {}",
            plan.working.display()
        );
        assert!(
            !plan.working.starts_with(&plan.staging),
            "and it must not hold open the staging folder it deletes either: {}",
            plan.working.display()
        );
        assert!(plan.working.is_dir(), "it has to be somewhere that exists");
        // And the spawn actually uses it. `begin_swap` is the only caller, and on this platform it
        // refuses rather than spawning — so the field is read off the source, which is the honest
        // way to assert a `#[cfg(windows)]` line from here (§267).
        let source = include_str!("update.rs");
        assert!(
            source.contains(".current_dir(&plan.working)"),
            "begin_swap no longer runs the helper outside the folder being replaced"
        );
    }

    /// The order **is** the safety property.
    #[test]
    fn nothing_moves_until_the_app_has_exited() {
        let script = swap_script(&a_plan());
        let waited = script.find("Get-Process").expect("it waits");
        let moved = script.find("Move-Item").expect("it moves");
        assert!(waited < moved, "the wait must come before the first move");

        let gave_up = script.find("still running").expect("it gives up");
        assert!(gave_up < moved, "and giving up must come before the first move too");
        assert!(script.contains("4242"), "it waits on this process");
    }

    /// The old folder is renamed aside, never deleted, because the rollback needs it back.
    #[test]
    fn the_old_build_is_retired_and_can_come_back() {
        let plan = a_plan();
        let script = swap_script(&plan);
        let retired = quote(&plan.retired);
        let install = quote(&plan.install);
        let staged = quote(&plan.staged);

        // Aside, then the new one in.
        let aside = script
            .find(&format!("Move-Item -LiteralPath {install} -Destination {retired}"))
            .expect("the old folder moves aside");
        let arrived = script
            .find(&format!("Move-Item -LiteralPath {staged} -Destination {install}"))
            .expect("the new folder moves in");
        assert!(aside < arrived);

        // And the rollback: the old one goes back, after the failure it recovers from.
        let back = script
            .find(&format!("Move-Item -LiteralPath {retired} -Destination {install}"))
            .expect("the old folder can come back");
        assert!(arrived < back, "the rollback belongs in the failure branch, after the attempt");
    }

    /// The one line that would make this unrecoverable.
    #[test]
    fn the_install_is_never_deleted_only_moved() {
        let plan = a_plan();
        let script = swap_script(&plan);
        let install = quote(&plan.install);
        assert!(
            !script.contains(&format!("Remove-Item -LiteralPath {install}")),
            "deleting the install rather than renaming it would remove the only rollback there is"
        );
        // What *is* removed: the two temporary folders, and only after the launch.
        let launched = script.find("Start-Process").expect("it launches");
        let cleaned = script.find("-Recurse -Force -ErrorAction SilentlyContinue").expect("cleanup");
        assert!(
            launched < cleaned,
            "a failed cleanup must not stop the new build from starting"
        );
    }

    /// A folder name is data. PowerShell expands nothing inside single quotes, and the only escape
    /// that applies is doubling — so a researcher whose username contains a quote is not a bug.
    #[test]
    fn a_path_is_quoted_as_data_not_code() {
        assert_eq!(quote(Path::new(r"C:\Users\A B\app")), r"'C:\Users\A B\app'");
        assert_eq!(quote(Path::new("/home/o'brien/app")), "'/home/o''brien/app'");
        // `$env:X` and a backtick-n stay literal inside single quotes, which is the point.
        let script = swap_script(&Swap::plan(
            1,
            Path::new("C:/Users/$env:USERNAME/app"),
            Path::new("C:/Users/$env:USERNAME/.mini-me-update-9.9.9/mini-me-desktop"),
            "9.9.9",
        ));
        assert!(script.contains("'C:/Users/$env:USERNAME/app'"), "{script}");
    }

    /// The script survives the trip to PowerShell byte for byte.
    ///
    /// `-Command` did not: the spawn succeeded, PowerShell started, and **nothing was written at
    /// all** — not even the log's first line, because PowerShell parses the whole string before
    /// running any of it. One mangled quote silences everything. `-EncodedCommand` is opaque
    /// base64, so nothing between here and the parser can reinterpret a character.
    #[test]
    fn the_script_arrives_exactly_as_it_was_written() {
        let script = swap_script(&a_plan());
        let encoded = encoded_script(&script);

        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .expect("valid base64");
        // UTF-16LE, which is what `-EncodedCommand` requires: two bytes per unit, low byte first.
        assert_eq!(bytes.len() % 2, 0, "UTF-16 is two bytes per unit");
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(String::from_utf16(&units).expect("valid UTF-16"), script);

        // The quotes that `-Command` could not carry are in there, unaltered.
        assert!(script.contains('"'), "the script does have double quotes to protect");
        assert!(encoded.chars().all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c)),
            "base64 only, so the command line cannot reinterpret anything");
    }

    /// `scripts/swap-rehearsal.ps1` must carry *this* script, not one from a week ago.
    ///
    /// A rehearsal of the wrong script is worse than none: it would prove something true of code
    /// that no longer ships, and it is being run precisely when nothing else can be trusted.
    #[test]
    fn the_rehearsal_runs_the_script_this_module_generates() {
        let template = swap_script(&Swap {
            pid: 999_999,
            install: PathBuf::from("__BASE__/mini-me-desktop"),
            staged: PathBuf::from("__BASE__/.mini-me-update-9.9.9/mini-me-desktop"),
            retired: PathBuf::from("__BASE__/.mini-me-previous-9.9.9"),
            staging: PathBuf::from("__BASE__/.mini-me-update-9.9.9"),
            launch: PathBuf::from("__BASE__/mini-me-desktop/mini-me-desktop-app.exe"),
            log: PathBuf::from("__LOG__"),
            working: PathBuf::from("__WORK__"),
        });
        let rehearsal = include_str!("../../../scripts/swap-rehearsal.ps1");
        assert!(
            rehearsal.contains(template.trim()),
            "scripts/swap-rehearsal.ps1 no longer carries the script this module generates, so \
             running it would prove nothing about the app"
        );
        // And it substitutes all three, or it would rehearse against literal placeholders.
        for placeholder in ["__BASE__", "__LOG__", "__WORK__"] {
            assert!(
                rehearsal.contains(&format!("Replace('{placeholder}'")),
                "{placeholder} is never substituted"
            );
        }
    }

    /// The helper is spawned the way PowerShell is already spawned successfully in this app.
    ///
    /// `notify.rs` has run PowerShell with `CREATE_NO_WINDOW` since §265. The updater reached for
    /// `DETACHED_PROCESS` instead, on the reasoning that the helper must outlive the app — which is
    /// true, and which nothing on Windows threatens: a child is not killed when its parent exits.
    /// What `DETACHED_PROCESS` actually did was leave a console application with no console, and it
    /// died before writing a byte.
    ///
    /// Two spawns of the same program in one binary should not disagree about how to start it.
    #[test]
    fn the_updater_and_the_notifier_start_powershell_the_same_way() {
        let flags_in = |source: &str| -> String {
            source
                .split("creation_flags(")
                .nth(1)
                .expect("a spawn")
                .split(')')
                .next()
                .expect("a flag expression")
                .trim()
                .to_string()
        };
        assert_eq!(
            flags_in(include_str!("update.rs")),
            flags_in(include_str!("notify.rs")),
            "the updater spawns PowerShell differently from the notifier, and only one of the two \
             has ever been seen to work"
        );
    }

    /// The helper must never open the log it is already holding.
    ///
    /// `begin_swap` hands the child the log as stdout and stderr, so the child *has* that file
    /// open. `Out-File` on the same path asks for a share mode the existing handle contradicts, and
    /// the open fails — which, under `$ErrorActionPreference = 'Stop'`, killed the script on its
    /// first statement. Three presses died there.
    ///
    /// The log path must therefore not appear in the script at all: Rust owns the handle, the
    /// script writes to the stream.
    #[test]
    fn the_script_writes_to_its_output_stream_not_to_the_log_file() {
        let plan = a_plan();
        let script = swap_script(&plan);
        assert!(
            !script.contains(&plan.log.to_string_lossy().to_string()),
            "the script names the log file, so it would try to open a file it already holds: \
             {script}"
        );
        assert!(!script.contains("Out-File"), "one writer, one handle: {script}");
        assert!(
            script.contains("function Note($m) { Write-Output"),
            "the helper must report through the stream Rust redirected: {script}"
        );
    }

    /// An empty log must not be ambiguous.
    ///
    /// "The helper never started" and "the helper started and said nothing" want opposite fixes,
    /// and an empty file looks the same either way. This process writes one line of its own before
    /// the child exists, so the file's *contents* answer which — a diagnostic that only helps when
    /// something has already gone wrong, which is when every other signal has failed (§272).
    #[test]
    fn the_log_says_the_spawn_was_reached_before_the_child_can_say_anything() {
        let source = include_str!("update.rs");
        let before = source.find("about to start the helper").expect("a line of our own");
        let spawn = source.find(".spawn()").expect("the spawn");
        assert!(before < spawn, "our line must be written before the child can write anything");
        assert!(
            source[before..spawn].contains("record.flush()"),
            "an unflushed line is no line at all when the process is about to exit"
        );
    }

    /// The label comes off the plan rather than beside it.
    #[test]
    fn the_log_line_names_the_staging_folder_it_is_about() {
        let plan = a_plan();
        assert_eq!(plan.tag_hint(), ".mini-me-update-0.3.1");
        assert_eq!(
            plan.staging.file_name().expect("a name").to_string_lossy(),
            plan.tag_hint(),
            "derived, so it cannot disagree with the rest of the plan"
        );
    }

    /// The failure that leaves nothing behind must stop being possible.
    ///
    /// Twice a press produced no restart, no log and no error, because the helper died before its
    /// first statement and — being detached — had no console for the message to land on. The child
    /// is now handed the log as its own stdout and stderr, so even a script that never parses
    /// writes *something*.
    #[test]
    fn the_helper_cannot_fail_without_leaving_a_trace() {
        let source = include_str!("update.rs");
        for wiring in [".stdout(record)", ".stderr(errors)"] {
            assert!(
                source.contains(wiring),
                "the helper no longer sends {wiring} to the update log, so a failure before its \
                 first statement would again leave nothing to read"
            );
        }
    }

    /// The command that carries it, and the two flags the swap cannot work without.
    #[test]
    fn the_helper_runs_without_a_profile_or_a_prompt() {
        let argv = swap_command(&a_plan());
        assert_eq!(argv[0], "powershell");
        assert!(argv.contains(&"-NoProfile".to_string()), "a profile script must not break it");
        assert!(
            argv.contains(&"-NonInteractive".to_string()),
            "a prompt nobody can answer would hang the swap forever"
        );
        assert!(argv.contains(&"Hidden".to_string()));
        // Encoded, not raw. `-Command` could not carry this script intact (§270), so the last
        // argument is base64 and the flag beside it says so.
        assert!(
            argv.contains(&"-EncodedCommand".to_string()),
            "a raw -Command string is what silently failed twice"
        );
        assert!(!argv.contains(&"-Command".to_string()));
        assert_eq!(
            argv.last().expect("a script"),
            &encoded_script(&swap_script(&a_plan()))
        );
    }

    /// The helper's log is the only record of a swap, so the Setup pane must name *that* file.
    ///
    /// §250 is the reason this is a test and not a comment: a researcher was handed a `/tmp` path
    /// for a file the app writes to `%TEMP%`, and the round trip came back empty. This log is
    /// worse than that one — it is written after the app has exited, so if the pane names the
    /// wrong file there is nothing else that could have recorded what happened.
    #[test]
    fn the_setup_pane_names_the_log_the_helper_writes() {
        let plan = a_plan();
        let name = plan
            .log
            .file_name()
            .expect("the helper logs somewhere")
            .to_string_lossy()
            .into_owned();
        let pane = include_str!("main.rs");
        assert!(
            pane.contains(&name),
            "the Setup pane does not name {name}, so a failed swap leaves nothing findable"
        );
        // Not the script: it must *not* name the log, because it must not open a file it already
        // holds as stdout (§274). The binding that matters is Rust's, so assert on that.
        assert!(
            include_str!("update.rs").contains(".open(&plan.log)"),
            "begin_swap no longer opens {name}, so nothing points the helper's output at it"
        );
    }

    /// The endpoint is this repository's, and it is asked without credentials.
    #[test]
    fn the_url_points_at_this_repository_and_asks_for_the_latest() {
        assert!(LATEST_URL.starts_with("https://api.github.com/repos/"));
        assert!(LATEST_URL.contains("CENTRO-INTERNACIONAL-DE-LA-PAPA/mini-me-desktop"));
        assert!(LATEST_URL.ends_with("/releases/latest"));
    }
}
