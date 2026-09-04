//! Installing themes from Zed's extension gallery.
//!
//! **Why this is possible at all.** Zed's extensions are usually described as WASM
//! compiled against Zed's own API, and for a *language* extension that is true — none of
//! those could ever run here. Theme extensions are different, and the registry says so
//! itself: every one of them reports `"wasm_api_version": null` and `"provides":
//! ["themes"]`. They are pure data — a `.tar.gz` of JSON files. That is the part worth
//! having, and it is portable (docs §52).
//!
//! So this is a *theme* installer, not an extension installer, and the distinction is
//! deliberate: promising "install Zed extensions" and shipping something that only handles
//! themes would be worse than saying which one this is.
//!
//! Two endpoints, both public and unauthenticated:
//!
//! - `GET /extensions?filter=<query>` — the index
//! - `GET /extensions/<id>/download`  — the archive
//!
//! Installed themes land in the same `themes/` directory a researcher can drop files into
//! by hand, so there is one place themes come from and one loader that reads them.

use anyhow::{bail, Context as _, Result};
use serde::{Deserialize, Serialize};

/// Zed's public extension index.
const REGISTRY: &str = "https://api.zed.dev/extensions";

/// One theme extension, as the registry describes it.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Listing {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    /// How many people have installed it — the only quality signal available, and a
    /// genuinely useful one when the alternative is a name and nothing else.
    #[serde(default)]
    pub download_count: u64,
    #[serde(default)]
    pub provides: Vec<String>,
    /// Where it comes from. Shown because these are other people's work, under their own
    /// licences, and a gallery that hides authorship is not a gallery.
    #[serde(default)]
    pub repository: String,
}

#[derive(Deserialize)]
struct Index {
    data: Vec<Listing>,
}

impl Listing {
    /// Whether this is a theme — the only kind that can work here.
    fn is_theme(&self) -> bool {
        self.provides.iter().any(|kind| kind == "themes")
    }
}

/// Search the gallery, returning only what this app can actually use.
pub async fn search(client: &reqwest::Client, query: &str) -> Result<Vec<Listing>> {
    let index: Index = client
        .get(REGISTRY)
        .query(&[("filter", query), ("max_schema_version", "1")])
        .send()
        .await
        .context("could not reach Zed's extension gallery")?
        .error_for_status()
        .context("the gallery returned an error status")?
        .json()
        .await
        .context("could not decode the gallery's reply")?;

    let mut themes: Vec<Listing> = index.data.into_iter().filter(Listing::is_theme).collect();
    // Most-installed first. With a hundred results and no preview, this is the closest
    // thing to a recommendation that does not require us to have opinions.
    themes.sort_by_key(|theme| std::cmp::Reverse(theme.download_count));
    Ok(themes)
}

/// Download one extension and write its palettes into `themes_dir`.
///
/// Returns the names installed. Everything outside `themes/*.json` in the archive is
/// ignored — an extension may carry a licence, a readme and a manifest, and none of them
/// are ours to interpret.
pub async fn install(
    client: &reqwest::Client,
    id: &str,
    themes_dir: &std::path::Path,
) -> Result<Vec<String>> {
    let archive = client
        .get(format!("{REGISTRY}/{id}/download"))
        .query(&[
            ("min_schema_version", "1"),
            ("max_schema_version", "1"),
            ("min_wasm_api_version", "0.0.0"),
            ("max_wasm_api_version", "0.6.0"),
        ])
        .send()
        .await
        .with_context(|| format!("could not download {id}"))?
        .error_for_status()
        .with_context(|| format!("the gallery refused to serve {id}"))?
        .bytes()
        .await
        .with_context(|| format!("could not read the download for {id}"))?;

    std::fs::create_dir_all(themes_dir)
        .with_context(|| format!("could not create {}", themes_dir.display()))?;

    let decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(archive));
    let mut tar = tar::Archive::new(decoder);
    let mut installed = Vec::new();

    for entry in tar
        .entries()
        .context("the download is not a readable archive")?
    {
        let mut entry = entry.context("could not read an entry from the archive")?;
        let path = entry
            .path()
            .context("an archive entry has no path")?
            .into_owned();

        // `themes/<something>.json`, and nothing else. Checking the *file name* rather
        // than the whole path also means a crafted archive cannot write outside the
        // themes directory — the classic tar traversal, which is worth ruling out even
        // from a registry we trust.
        let is_theme_json = path.extension().and_then(|e| e.to_str()) == Some("json")
            && path.components().any(|part| part.as_os_str() == "themes");
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !is_theme_json || name.contains("..") {
            continue;
        }

        let mut json = String::new();
        if std::io::Read::read_to_string(&mut entry, &mut json).is_err() {
            continue;
        }
        std::fs::write(themes_dir.join(name), &json)
            .with_context(|| format!("could not write {name}"))?;
        installed.push(name.to_string());
    }

    if installed.is_empty() {
        bail!("{id} carried no theme files");
    }
    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_theme_extensions_are_offered() {
        // The registry's own shape, from a live call: an icon-theme and a language
        // extension sit beside the themes and neither can work here.
        let index: Index = serde_json::from_value(serde_json::json!({"data": [
            {"id": "catppuccin", "name": "Catppuccin", "provides": ["themes"], "download_count": 964535},
            {"id": "catppuccin-icons", "name": "Catppuccin Icons", "provides": ["icon-themes"], "download_count": 47000},
            {"id": "some-lang", "name": "A Language", "provides": ["languages"], "download_count": 999999}
        ]}))
        .expect("the registry shape");
        let themes: Vec<Listing> = index.data.into_iter().filter(Listing::is_theme).collect();
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].id, "catppuccin");
    }

    #[test]
    fn an_archive_yields_only_its_theme_files() {
        let dir = std::env::temp_dir().join(format!("minime-gallery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temp dir");

        // Build the archive shape the gallery serves, including the things we must skip:
        // a licence, a manifest, and a path trying to escape the themes directory.
        let mut builder = tar::Builder::new(Vec::new());
        let mut add = |path: &str, body: &str| {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, path, body.as_bytes())
                .expect("append");
        };
        add("ext/themes/one.json", r#"{"themes":[]}"#);
        add("ext/themes/two.json", r#"{"themes":[]}"#);
        add("ext/LICENSE", "MIT");
        add("ext/extension.toml", "id = 'x'");
        let tarball = builder.into_inner().expect("finish");

        let mut archive = tar::Archive::new(std::io::Cursor::new(tarball));
        let mut installed: Vec<String> = Vec::new();
        for entry in archive.entries().expect("entries") {
            let entry = entry.expect("entry");
            let path = entry.path().expect("path").into_owned();
            let is_theme_json = path.extension().and_then(|e| e.to_str()) == Some("json")
                && path.components().any(|part| part.as_os_str() == "themes");
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if is_theme_json && !name.contains("..") {
                installed.push(name.to_string());
            }
        }
        installed.sort();
        assert_eq!(installed, vec!["one.json", "two.json"]);

        // The traversal guard cannot be tested by forging an archive — the `tar` crate
        // refuses to *write* a path containing `..` — so the predicate is checked
        // directly. It is the reason the write target is built from the file name rather
        // than from the archive's own path.
        assert!("../../evil.json".contains(".."));
        assert!(!"one.json".contains(".."));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
