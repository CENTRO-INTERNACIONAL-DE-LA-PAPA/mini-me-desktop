//! Check that a citation's DOI points at the paper the citation names.
//!
//! # Why this exists
//!
//! Five references from one real run were checked by hand against Crossref (docs §119). Three
//! DOIs resolved to real papers that were **not** the ones cited — one to a study of lichen
//! symbioses, one to a different chapter of the right book, one to a different article in the
//! right issue of the right journal. Two did not resolve at all.
//!
//! The pattern is worth stating, because it is what makes this worth automating. In the case that
//! could be checked most precisely, the model had the paper *right*: Hijmans & Spooner 2001,
//! American Journal of Botany, **88(11), 2101-2112** — volume, issue and pages all correct. Only
//! the DOI was wrong, and the wrong one was a real DOI in the same journal and year.
//!
//! So every field a person would sanity-check comes out plausible. A DOI suffix is a
//! high-entropy string carrying no meaning, which makes it the first thing a language model loses
//! and the last thing a reader can verify by eye. That asymmetry is the whole argument for doing
//! it in software.
//!
//! # What leaves the machine
//!
//! **A DOI, and nothing else.** Not the citation, not the question that produced it, not the
//! conversation. A DOI is a public identifier for a published work, which is the one part of this
//! that is already world-readable — and the check is a lookup *by* that identifier, with the
//! comparison done here against text that never leaves. Org policy is explicit that confidential
//! and unpublished material must not be sent to third parties; this sends neither.
//!
//! # Why Crossref rather than Semantic Scholar
//!
//! Crossref is the registrar: a DOI exists because it was registered there, so "this DOI does not
//! resolve" is a fact it is authoritative about. Semantic Scholar is an index, and its coverage
//! has real gaps — two of the five above are absent from S2 *and* absent from Crossref, but only
//! the second absence proves anything. Using the index to answer a question about the registry is
//! how you get a false negative on a book chapter.

/// What checking one reference found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// It resolves, and the title it resolves to is the one cited.
    Confirmed,
    /// It resolves to a **different** work. The worst kind, because the link opens something
    /// plausible and a reader has no reason to look further.
    Mismatch { found: String },
    /// The registry has no such DOI. It was not mistyped from a real one — it is not one.
    Unregistered,
    /// Nothing to check: no DOI in the citation and none in the structured field.
    NoIdentifier,
    /// The check itself failed — offline, rate-limited, or the service is down. **Not** a
    /// verdict about the reference, and must never be shown as one.
    Unreachable { why: String },
}

impl Verdict {
    /// Whether this says something is wrong with the *reference*.
    pub fn is_problem(&self) -> bool {
        matches!(self, Verdict::Mismatch { .. } | Verdict::Unregistered)
    }

    /// One line, in the researcher's terms.
    pub fn label(&self) -> String {
        match self {
            Verdict::Confirmed => "DOI checked — resolves to this paper".to_string(),
            Verdict::Mismatch { found } => format!("DOI resolves to a different paper: {found}"),
            Verdict::Unregistered => "this DOI is not registered — no such record".to_string(),
            Verdict::NoIdentifier => "no DOI to check".to_string(),
            Verdict::Unreachable { why } => format!("could not check ({why})"),
        }
    }
}

/// The bare DOI inside a link, or `None` if it carries none.
///
/// Handles the forms these arrive in: a `doi.org` URL, the `dx.doi.org` host that predates it,
/// and a bare `10.…` identifier. An arXiv or Semantic Scholar URL yields `None` — they are
/// perfectly good links, but Crossref cannot answer questions about them, and returning something
/// here that then fails to resolve would report a real paper as unregistered.
pub fn doi_in(link: &str) -> Option<String> {
    let link = link.trim();
    let rest = link
        .split_once("doi.org/")
        .map(|(_, rest)| rest)
        .unwrap_or(link);
    let rest = rest.trim_start_matches("doi:").trim();
    // Every registered DOI starts `10.` followed by a registrant code and a slash.
    if !rest.starts_with("10.") || !rest.contains('/') {
        return None;
    }
    // A URL fragment or query is not part of the identifier.
    let bare = rest
        .split(['#', '?'])
        .next()
        .unwrap_or(rest)
        .trim_end_matches(['.', ',', ')']);
    (bare.len() > 4).then(|| bare.to_string())
}

/// Words too common to carry any evidence that two titles are the same work.
const NOISE: [&str; 24] = [
    "a", "an", "and", "as", "at", "by", "for", "from", "in", "is", "of", "on", "or", "the", "to",
    "with", "into", "its", "their", "this", "that", "using", "via", "between",
];

/// A title reduced to the words worth comparing.
fn significant(text: &str) -> Vec<String> {
    text.to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        // Two-letter fragments are journal abbreviations and initials far more often than
        // they are title words.
        .filter(|word| word.len() > 2 && !NOISE.contains(word))
        .map(str::to_string)
        .collect()
}

/// How much of the registry's title appears in the citation, from 0 to 1.
///
/// Directional on purpose. The citation is a whole reference — authors, year, journal, pages —
/// so the *title* is a subset of it, and asking "is the registry's title inside this citation"
/// is the question. The reverse would score every correct reference badly for containing an
/// author list.
pub fn overlap(citation: &str, title: &str) -> f32 {
    let wanted = significant(title);
    if wanted.is_empty() {
        return 0.;
    }
    let have: std::collections::HashSet<String> = significant(citation).into_iter().collect();
    let found = wanted.iter().filter(|word| have.contains(*word)).count();
    found as f32 / wanted.len() as f32
}

/// Enough of the title present to call it the same work.
///
/// 0.6 rather than something stricter because a citation legitimately shortens a title — a
/// subtitle dropped after a colon, "Phytophthora infestans" left out of the parenthetical. And
/// not lower, because the wrong papers in §119 scored 0.27 and 0.20: real mismatches sit far
/// below this line, so the exact threshold is not load-bearing.
const SAME_WORK: f32 = 0.6;

/// Compare a citation against the title the registry returned.
pub fn judge(citation: &str, registry_title: &str) -> Verdict {
    if overlap(citation, registry_title) >= SAME_WORK {
        Verdict::Confirmed
    } else {
        Verdict::Mismatch {
            found: registry_title.to_string(),
        }
    }
}

/// Pull the work's title out of a Crossref `/works/{doi}` response.
pub fn title_of(body: &serde_json::Value) -> Option<String> {
    let title = body
        .get("message")?
        .get("title")?
        .as_array()?
        .first()?
        .as_str()?
        .trim();
    (!title.is_empty()).then(|| title.split_whitespace().collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_doi_is_recognised_in_the_forms_it_arrives_in() {
        for link in [
            "https://doi.org/10.2307/3558435",
            "http://dx.doi.org/10.2307/3558435",
            "10.2307/3558435",
            "doi:10.2307/3558435",
            "https://doi.org/10.2307/3558435#abstract",
            "https://doi.org/10.2307/3558435.",
        ] {
            assert_eq!(
                doi_in(link).as_deref(),
                Some("10.2307/3558435"),
                "{link}"
            );
        }
        // Real links Crossref cannot answer for. Returning a DOI-shaped nothing here would
        // report a genuine arXiv paper as unregistered.
        assert_eq!(doi_in("https://arxiv.org/abs/2510.21652"), None);
        assert_eq!(doi_in("https://api.semanticscholar.org/CorpusID:906398"), None);
        assert_eq!(doi_in("https://example.org/paper"), None);
        assert_eq!(doi_in(""), None);
        assert_eq!(doi_in("10.2307"), None, "no slash, not a DOI");
    }

    /// The five real references from §119, with what Crossref actually returned for each.
    #[test]
    fn the_five_real_references_come_out_the_way_they_were_checked_by_hand() {
        // Right paper: the citation and the registry agree.
        assert_eq!(
            judge(
                "Hijmans, R.J., & Spooner, D.M. (2001). Geographic distribution of wild potato \
                 species. American Journal of Botany, 88(11), 2101-2112.",
                "Geographic distribution of wild potato species"
            ),
            Verdict::Confirmed
        );

        // What the DOI the model wrote actually resolves to.
        let lichen = judge(
            "Hijmans, R.J., & Spooner, D.M. (2001). Geographic distribution of wild potato \
             species. American Journal of Botany, 88(11), 2101-2112.",
            "Algal switching among lichen symbioses",
        );
        assert!(lichen.is_problem(), "{lichen:?}");

        // Right journal, right issue, different article — the case a reader cannot catch, since
        // the volume and issue in the citation are correct.
        let aphids = judge(
            "Vargas, P., Forbes, G.A., & Mendoza, H. (2012). Characterization of quantitative \
             resistance to late blight in Peruvian landrace potatoes. American Journal of Potato \
             Research, 89(6), 444-453.",
            "Resistance to Aphids, Late Blight and Viruses in Somatic Fusions and Crosses of \
             Solanum tuberosum L. and Solanum bulbocastanum Dun",
        );
        assert!(aphids.is_problem(), "{aphids:?}");

        // Right book, wrong chapter.
        let gender = judge(
            "Lindqvist-Kreuze, H., & Forbes, G.A. (2018). Genotype × environment interactions and \
             pathogen race diversity: Implications for breeding for durability in catastrophic \
             diseases (Chapter in: The Potato Crop, Springer, pp. 467-486).",
            "Gender Topics on Potato Research and Development",
        );
        assert!(gender.is_problem(), "{gender:?}");
    }

    #[test]
    fn a_shortened_title_is_still_the_same_work() {
        // A citation may drop a subtitle or a parenthetical. Flagging that would make the check
        // noise, and a check people learn to ignore is worse than none.
        assert_eq!(
            judge(
                "Kirk, W.W. et al. (2001). Effect of host plant resistance and reduced rates of \
                 fungicide application to control potato late blight. Plant Disease 85(10).",
                "Effect of Host Plant Resistance and Reduced Rates and Frequencies of Fungicide \
                 Application to Control Potato Late Blight"
            ),
            Verdict::Confirmed
        );
        // Case and punctuation carry no weight.
        assert_eq!(
            judge("SMITH (2020). THE POTATO CROP: A HANDBOOK.", "The potato crop — a handbook"),
            Verdict::Confirmed
        );

        // The real mismatches sit far below the threshold, so its exact value is not
        // load-bearing. If this ever gets close, the rule needs rethinking rather than tuning.
        let wrong = overlap(
            "Hijmans & Spooner (2001). Geographic distribution of wild potato species.",
            "Algal switching among lichen symbioses",
        );
        assert!(wrong < 0.25, "a real mismatch scored {wrong}");
    }

    #[test]
    fn a_crossref_body_yields_its_title() {
        let body = serde_json::json!({
            "status": "ok",
            "message": {
                "title": ["Geographic distribution of wild\n  potato species"],
                "container-title": ["American Journal of Botany"],
            }
        });
        assert_eq!(
            title_of(&body).as_deref(),
            Some("Geographic distribution of wild potato species"),
            "whitespace in the registry's own record is normalised"
        );
        // A shape we do not understand is not a title, and must not become an empty mismatch.
        assert_eq!(title_of(&serde_json::json!({"message": {}})), None);
        assert_eq!(title_of(&serde_json::json!({"message": {"title": []}})), None);
        assert_eq!(title_of(&serde_json::json!({})), None);
    }

    #[test]
    fn only_a_reference_problem_is_a_problem() {
        // Being offline says nothing about a citation, and must never be shown as though it did.
        assert!(!Verdict::Unreachable {
            why: "offline".into()
        }
        .is_problem());
        assert!(!Verdict::NoIdentifier.is_problem());
        assert!(!Verdict::Confirmed.is_problem());
        assert!(Verdict::Unregistered.is_problem());
    }
}
