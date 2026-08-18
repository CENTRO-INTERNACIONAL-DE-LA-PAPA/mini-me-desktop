//! Fetching a CIP Dataverse dataset into the researcher's own folder.
//!
//! # Why this is not the MCP's job
//!
//! The Dataverse MCP has a download tool and it is useless to us. Probed against the live server
//! (docs §223):
//!
//! ```text
//! download_dataset_files_by_doi(doi, output_dir, extract_zip)
//!   output_dir: "Directory to save downloaded files. Defaults to a server-managed directory."
//! ```
//!
//! That directory is on `dataverse-cip.fastmcp.app`, so the tool downloads a dataset onto somebody
//! else's machine and reports success. Turning it on would give the subagent a way to say it
//! fetched a file nobody can open. The skill's own reference already said as much — *"those files
//! do not automatically appear inside the sandbox"* — which on the web was the end of the matter.
//!
//! On the desktop it is not, because of one fact that took a while to notice: **the thread's
//! folder is the sandbox's working directory**. `crate::workspace` puts it on the Windows side at
//! `Documents\Mini-Me\<thread>` and the backend writes there through `MINIME_LOCAL_WORKSPACE`. So
//! a file the app downloads is a file `data_cleaning`, EDA and DataVoyager can already open — the
//! loop closes without the agent being handed a network tool at all.
//!
//! # Open datasets only, and not on the model's word
//!
//! *"we should only be able to download open datasets not restricted."*
//!
//! `DataVerseFindings` has a `file_access_summary` field, and it would be the wrong thing to gate
//! on: it is prose a model wrote. The search results carry no access field either — twenty-six
//! keys per row, none of them about restriction. The trustworthy signal needs a second call, to
//! the files API, which answers `restricted` per file as a boolean the server owns
//! (`ls /api/datasets/:persistentId/versions/:latest/files`).
//!
//! So [`Access::of`] asks, and a dataset with any restricted file is refused with its reason
//! rather than hidden. The researcher keeps the knowledge that the data exists and can be
//! requested from CIP; what they do not get is a partial download that looks like the whole thing.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _, Result};
use serde::Deserialize;

/// The Dataverse instance this app talks to.
///
/// Hardcoded because the whole subagent is: its search tool is literally `SearchCIPDataverse` and
/// the skill states *"this skill is currently scoped to CIP Dataverse"*. Resolved from a dataset
/// DOI (`doi.org/10.21223/…` → here), since it appears in neither repository — the MCP keeps it in
/// a `DATAVERSE_BASE_URL` of its own that it never returns.
///
/// The variable exists so a second instance can be tried without a rebuild, not because we support
/// one: [`instance`] is the only reader.
pub const INSTANCE_ENV: &str = "MINIME_DATAVERSE_URL";
const DEFAULT_INSTANCE: &str = "https://data.cipotato.org";

/// Refuse a dataset larger than this rather than filling somebody's disk from a button press.
///
/// The files API reports every file's size before anything is fetched, so this is checked against
/// a number the server gave us rather than discovered halfway through a download.
pub const SIZE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

pub fn instance() -> String {
    std::env::var(INSTANCE_ENV)
        .ok()
        .map(|url| url.trim().trim_end_matches('/').to_string())
        .filter(|url| url.starts_with("http"))
        .unwrap_or_else(|| DEFAULT_INSTANCE.to_string())
}

/// One file inside a dataset, as the server describes it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct DatasetFile {
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub size: u64,
    /// **The gate.** A boolean the server owns, not a sentence a model wrote.
    #[serde(default)]
    pub restricted: bool,
}

/// What the files API said about a dataset, and what follows from it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Access {
    pub files: Vec<DatasetFile>,
}

/// Why a dataset cannot be fetched, in the words the row will show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// At least one file is restricted. Named individually, because "3 of 6" is the difference
    /// between "ask CIP for access" and "this is the wrong dataset".
    Restricted { count: usize, of: usize },
    /// Bigger than [`SIZE_LIMIT`].
    TooLarge { bytes: u64 },
    /// The version has no files at all — a metadata-only or draft record.
    Empty,
}

impl Refusal {
    pub fn reason(&self) -> String {
        match self {
            Refusal::Restricted { count, of } => format!(
                "{count} of {of} files are restricted — request access from CIP to download this"
            ),
            Refusal::TooLarge { bytes } => format!(
                "{} is over the {} download limit — open the dataset page to fetch it directly",
                human_size(*bytes),
                human_size(SIZE_LIMIT)
            ),
            Refusal::Empty => "this version has no downloadable files".to_string(),
        }
    }
}

impl Access {
    /// Whether this dataset may be downloaded, and why not when it may not.
    ///
    /// Order matters and is deliberate: restriction is reported ahead of size, because "you may
    /// not have this" is a different message from "this is too big" and the researcher's next
    /// action differs. A dataset that is both gets the one they can act on.
    pub fn refusal(&self) -> Option<Refusal> {
        if self.files.is_empty() {
            return Some(Refusal::Empty);
        }
        let restricted = self.files.iter().filter(|file| file.restricted).count();
        if restricted > 0 {
            return Some(Refusal::Restricted {
                count: restricted,
                of: self.files.len(),
            });
        }
        let bytes = self.bytes();
        if bytes > SIZE_LIMIT {
            return Some(Refusal::TooLarge { bytes });
        }
        None
    }

    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }

    /// What the button says when the dataset can be had: how much is about to arrive.
    pub fn offer(&self) -> String {
        match self.files.len() {
            1 => format!("Download · {}", human_size(self.bytes())),
            n => format!("Download {n} files · {}", human_size(self.bytes())),
        }
    }

    /// Ask the server which files this dataset has and whether they are restricted.
    pub async fn of(client: &reqwest::Client, persistent_id: &str) -> Result<Access> {
        let response = client
            .get(format!("{}/api/datasets/:persistentId/versions/:latest/files", instance()))
            .query(&[("persistentId", persistent_id)])
            .send()
            .await
            .with_context(|| format!("asking {} which files {persistent_id} has", instance()))?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("{} answered {status} for {persistent_id}", instance());
        }
        Ok(Access {
            files: parse_files(&body)?,
        })
    }
}

/// The `data` array of a Dataverse `files` response, whichever nesting it arrived in.
///
/// Native Dataverse wraps each file in a `dataFile` object and puts `restricted` on the *outer*
/// entry; the MCP flattens both into one. Both are read, and `restricted` is taken from wherever
/// it appears — a shape that parsed but lost the restriction flag would produce the one wrong
/// answer this module exists to prevent.
fn parse_files(body: &str) -> Result<Vec<DatasetFile>> {
    let value: serde_json::Value =
        serde_json::from_str(body).context("reading the file list as JSON")?;
    let entries = value
        .get("data")
        .or_else(|| value.get("files"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(entries
        .iter()
        .map(|entry| {
            let inner = entry.get("dataFile").unwrap_or(entry);
            let text = |key: &str| {
                inner
                    .get(key)
                    .or_else(|| entry.get(key))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            DatasetFile {
                filename: match text("filename") {
                    name if name.is_empty() => text("label"),
                    name => name,
                },
                content_type: match text("contentType") {
                    kind if kind.is_empty() => text("content_type"),
                    kind => kind,
                },
                size: inner
                    .get("filesize")
                    .or_else(|| inner.get("size"))
                    .or_else(|| entry.get("size"))
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or_default(),
                // Either level, and true if either says so.
                restricted: [entry.get("restricted"), inner.get("restricted")]
                    .into_iter()
                    .flatten()
                    .any(|flag| flag.as_bool().unwrap_or(false)),
            }
        })
        .collect())
}

/// A filename for the archive, derived from the identifier rather than the title.
///
/// Titles are prose and arrive from a model; identifiers are short, unique and already
/// filesystem-shaped once the two separators are replaced. It also means the file on disk names
/// the thing a citation names.
pub fn archive_name(persistent_id: &str) -> String {
    let stem: String = persistent_id
        .trim()
        .trim_start_matches("doi:")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let stem = stem.trim_matches('-').to_string();
    format!(
        "{}.zip",
        if stem.is_empty() { "dataset" } else { &stem }
    )
}

/// Fetch the dataset into `folder`, returning where it landed.
///
/// Checks access again rather than trusting the caller's earlier answer: the button was drawn from
/// a listing that may be minutes old, and the cost of being wrong here is downloading something
/// the researcher is not entitled to.
pub async fn download(
    client: &reqwest::Client,
    persistent_id: &str,
    folder: &Path,
) -> Result<PathBuf> {
    let access = Access::of(client, persistent_id).await?;
    if let Some(refusal) = access.refusal() {
        bail!("{}", refusal.reason());
    }

    let response = client
        .get(format!("{}/api/access/dataset/:persistentId/", instance()))
        .query(&[("persistentId", persistent_id)])
        .send()
        .await
        .with_context(|| format!("downloading {persistent_id}"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("{} answered {status} downloading {persistent_id}", instance());
    }
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("reading {persistent_id}"))?;

    std::fs::create_dir_all(folder)
        .with_context(|| format!("creating {}", folder.display()))?;
    let target = folder.join(archive_name(persistent_id));
    std::fs::write(&target, &bytes)
        .with_context(|| format!("writing {}", target.display()))?;
    Ok(target)
}

/// Bytes in the units a person reads.
pub fn human_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    match bytes {
        0 => "empty".to_string(),
        n if n < KB => format!("{n} B"),
        n if n < MB => format!("{:.0} KB", n as f64 / KB as f64),
        n if n < GB => format!("{:.1} MB", n as f64 / MB as f64),
        n => format!("{:.1} GB", n as f64 / GB as f64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real shape, captured from `list_dataset_files(doi:10.21223/P3/0F9T62)`.
    const MCP_SHAPE: &str = r#"{"status":"success","dataset_persistent_id":"doi:10.21223/P3/0F9T62",
        "files":[
          {"file_id":11652,"filename":"data_dictionary.csv","content_type":"text/comma-separated-values","size":6521,"restricted":false},
          {"file_id":11653,"filename":"epidemics.tab","content_type":"text/tab-separated-values","size":204800,"restricted":false}]}"#;

    /// Native Dataverse, which nests the file and puts `restricted` on the outer entry.
    const NATIVE_SHAPE: &str = r#"{"status":"OK","data":[
          {"label":"open.csv","restricted":false,"dataFile":{"filename":"open.csv","contentType":"text/csv","filesize":100}},
          {"label":"closed.csv","restricted":true,"dataFile":{"filename":"closed.csv","contentType":"text/csv","filesize":200}}]}"#;

    #[test]
    fn the_mcp_shape_is_read() {
        let files = parse_files(MCP_SHAPE).expect("parses");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].filename, "data_dictionary.csv");
        assert_eq!(files[0].size, 6521);
        assert!(!files[0].restricted);
    }

    /// **The one that matters.** `restricted` lives on the outer entry in native Dataverse, so a
    /// reader that only looked inside `dataFile` would call a closed dataset open — the single
    /// wrong answer this module must not give.
    #[test]
    fn a_restricted_file_is_seen_through_the_nesting() {
        let access = Access {
            files: parse_files(NATIVE_SHAPE).expect("parses"),
        };
        assert!(access.files[1].restricted);
        assert_eq!(
            access.refusal(),
            Some(Refusal::Restricted { count: 1, of: 2 })
        );
    }

    #[test]
    fn an_open_dataset_is_offered_with_what_it_will_cost() {
        let access = Access {
            files: parse_files(MCP_SHAPE).expect("parses"),
        };
        assert_eq!(access.refusal(), None);
        assert_eq!(access.offer(), "Download 2 files · 206 KB");
    }

    #[test]
    fn restriction_is_reported_ahead_of_size() {
        // A dataset that is both: the researcher can act on "ask for access", not on "too big".
        let access = Access {
            files: vec![
                DatasetFile {
                    filename: "huge.tab".into(),
                    size: SIZE_LIMIT + 1,
                    restricted: true,
                    ..Default::default()
                },
            ],
        };
        assert!(matches!(access.refusal(), Some(Refusal::Restricted { .. })));
    }

    #[test]
    fn a_metadata_only_record_says_so_rather_than_offering_nothing() {
        assert_eq!(Access::default().refusal(), Some(Refusal::Empty));
    }

    #[test]
    fn an_oversized_open_dataset_is_refused_with_its_size() {
        let access = Access {
            files: vec![DatasetFile {
                size: SIZE_LIMIT + 1,
                ..Default::default()
            }],
        };
        let Some(refusal) = access.refusal() else {
            panic!("must refuse");
        };
        assert!(refusal.reason().contains("over the"));
    }

    #[test]
    fn the_archive_is_named_after_the_identifier_not_the_title() {
        assert_eq!(archive_name("doi:10.21223/P3/0F9T62"), "10-21223-P3-0F9T62.zip");
        // A title-shaped id would still produce something a filesystem accepts.
        assert_eq!(archive_name("doi:10.5072/FK2/ABC DEF"), "10-5072-FK2-ABC-DEF.zip");
        assert_eq!(archive_name(""), "dataset.zip");
    }

    #[test]
    fn a_body_that_is_not_a_file_list_fails_rather_than_reading_as_open() {
        // An error page must not decode to "no files", which `refusal` would call `Empty` —
        // wrong, but at least refusing. Anything that is not JSON is an error outright.
        assert!(parse_files("<html>gateway timeout</html>").is_err());
        // Valid JSON with no `data`/`files` is an empty list, which `Empty` then refuses.
        assert_eq!(parse_files(r#"{"status":"ERROR"}"#).expect("parses").len(), 0);
    }

    #[test]
    fn sizes_read_the_way_a_person_says_them() {
        assert_eq!(human_size(0), "empty");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(6521), "6 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn the_instance_is_cip_unless_something_says_otherwise() {
        assert_eq!(instance(), DEFAULT_INSTANCE);
    }
}
