//! Finished Theorizer runs, read back from the conversation's own folder.
//!
//! When a run ends, the backend's poll route writes `theories/<task_id>.json` (and a `.md`
//! beside it) into the conversation's folder (`persist_theory_outputs` in
//! `backend/theory_tools.py`). That file is the only place the finished theories exist: the
//! conversation's `hypotheses` artifact keeps the `running` record it was given at launch, with
//! no theories in it. So the Pinboard reads the file rather than the artifact.
//!
//! Read-only. Nothing here changes how a run is launched, polled or saved.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The folder, inside a conversation's folder, that finished runs are written to.
const THEORIES_DIR: &str = "theories";

/// One finished Theorizer run, as its saved JSON describes it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct TheoryRun {
    pub question: String,
    pub task_id: String,
    pub theories: Vec<Theory>,
    pub knowledge_gaps: Vec<String>,
    /// Optional so a `null` here cannot cost the whole run its card.
    pub papers_reviewed: Option<u64>,
    /// The readable copy written beside the JSON, when it exists. Not part of the file.
    #[serde(skip)]
    pub report: Option<PathBuf>,
}

/// One theory. Every field is optional on the wire: older runs carry only `laws` and papers.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Theory {
    pub name: Option<String>,
    pub summary: Option<String>,
    pub laws: Vec<String>,
    pub supporting_papers: Vec<PaperRef>,
    pub conflicting_papers: Vec<PaperRef>,
    pub novelty_label: Option<String>,
    pub predictions: Vec<String>,
    pub negative_experiments: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct PaperRef {
    pub citation: String,
    pub url: Option<String>,
}

impl Theory {
    /// The theory's name, or its first law when the theorizer gave it no name.
    pub fn title(&self) -> String {
        non_empty(self.name.as_deref())
            .or_else(|| self.laws.first().map(String::as_str))
            .unwrap_or("Untitled theory")
            .to_string()
    }

    /// The theorizer's classification against established theory, as a short phrase:
    /// `somewhat-related-to-existing` reads "Somewhat related to existing".
    pub fn novelty(&self) -> Option<String> {
        let label = non_empty(self.novelty_label.as_deref())?.replace(['-', '_'], " ");
        let mut chars = label.chars();
        let first = chars.next()?;
        Some(first.to_uppercase().chain(chars).collect())
    }
}

fn non_empty(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|text| !text.is_empty())
}

/// Every finished run in `conversation`'s folder that produced at least one theory, newest first.
///
/// A run that ended with no theories is left out: its record already says so in the jobs list,
/// and an empty card would only repeat it. Unreadable files are skipped with a warning rather
/// than hiding the runs that did parse.
pub fn load(conversation: &Path) -> Vec<TheoryRun> {
    let Ok(entries) = std::fs::read_dir(conversation.join(THEORIES_DIR)) else {
        return Vec::new();
    };
    let mut runs: Vec<(std::time::SystemTime, TheoryRun)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path)
                .map_err(|error| tracing::warn!(%error, path = %path.display(), "could not read a theorizer result"))
                .ok()?;
            let mut run = parse(&text)
                .map_err(|error| tracing::warn!(%error, path = %path.display(), "could not parse a theorizer result"))
                .ok()?;
            if run.theories.is_empty() {
                return None;
            }
            let report = path.with_extension("md");
            run.report = report.is_file().then_some(report);
            let modified = std::fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            Some((modified, run))
        })
        .collect();
    runs.sort_by(|a, b| b.0.cmp(&a.0));
    runs.into_iter().map(|(_, run)| run).collect()
}

fn parse(text: &str) -> serde_json::Result<TheoryRun> {
    serde_json::from_str(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape `persist_theory_outputs` writes for a run under the theory_store format.
    #[test]
    fn a_saved_run_parses_with_every_field_the_card_shows() {
        let run = parse(
            r#"{"question": "Why X?", "status": "completed", "task_id": "t-1",
                "papers_reviewed": 20, "knowledge_gaps": ["gap"],
                "theories": [{"name": "Saliency", "summary": "Protect what viewers notice.",
                  "laws": ["a", "b"], "supporting_papers": [{"citation": "P", "url": "https://doi.org/1", "doi": "1"}],
                  "conflicting_papers": [], "novelty_score": null,
                  "novelty_label": "somewhat-related-to-existing", "related_work": ["r"],
                  "predictions": ["p"], "negative_experiments": ["n"]}]}"#,
        )
        .unwrap();
        assert_eq!(run.papers_reviewed, Some(20));
        let theory = &run.theories[0];
        assert_eq!(theory.title(), "Saliency");
        assert_eq!(
            theory.novelty().as_deref(),
            Some("Somewhat related to existing")
        );
        assert_eq!(
            theory.supporting_papers[0].url.as_deref(),
            Some("https://doi.org/1")
        );
        assert_eq!(theory.predictions, ["p"]);
        assert_eq!(theory.negative_experiments, ["n"]);
    }

    /// Runs saved before the theory_store fix carry only laws and papers.
    #[test]
    fn a_legacy_run_without_names_falls_back_to_its_first_law() {
        let run = parse(r#"{"theories": [{"laws": ["X inhibits Y"]}]}"#).unwrap();
        assert_eq!(run.theories[0].title(), "X inhibits Y");
        assert_eq!(run.theories[0].novelty(), None);
    }

    #[test]
    fn a_folder_without_runs_loads_nothing() {
        let dir = std::env::temp_dir().join(format!("minime-theories-{}", std::process::id()));
        assert!(load(&dir).is_empty());
    }
}
