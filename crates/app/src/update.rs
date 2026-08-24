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
    if BUNDLE_MARKERS
        .iter()
        .all(|marker| folder.join(marker).is_dir())
    {
        Layout::Packaged(folder.to_path_buf())
    } else {
        Layout::Source
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

    /// The endpoint is this repository's, and it is asked without credentials.
    #[test]
    fn the_url_points_at_this_repository_and_asks_for_the_latest() {
        assert!(LATEST_URL.starts_with("https://api.github.com/repos/"));
        assert!(LATEST_URL.contains("CENTRO-INTERNACIONAL-DE-LA-PAPA/mini-me-desktop"));
        assert!(LATEST_URL.ends_with("/releases/latest"));
    }
}
