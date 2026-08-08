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
//! # What leaves the machine, and when
//!
//! This runs **automatically**, as sources arrive, with no control to press — see
//! `Workbench::resolve_sources` for why. That makes the disclosure worth stating precisely rather
//! than leaving to a button someone chose to click:
//!
//! * A **DOI**, for every reference that carries one.
//! * The **citation text**, for a reference whose DOI turned out wrong or missing — because that
//!   text *is* the query that finds the real work.
//!
//! Both go to `crossref.org` and nowhere else. Never the researcher's question, never the
//! conversation, never a file, never anything the agent produced. A citation and a DOI both name
//! published work, which is the one part of this that is already world-readable. Org policy
//! forbids sending confidential or unpublished material to third parties; this sends neither.
//!
//! # What a negative result does *not* mean
//!
//! Crossref registers **journal articles**. Books, monographs, society series and much grey
//! literature are largely absent from it, and so are older works generally — Sørensen's 1948
//! similarity index, one of the most cited works in plant ecology, has no DOI at all.
//!
//! So "nothing matches" is a fact about the registry, not about the world, and this module says
//! so in those words. The first version of the message read *"does not appear to describe a real
//! paper"*, which would have told a researcher that a correctly cited monograph was fabricated —
//! worse than the fabrications the check exists to catch, because it arrives with the app's
//! authority rather than the model's.
//!
//! The rule, and it cuts both ways: **report what was checked, not what was concluded.**
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
    /// The link is a Semantic Scholar corpus id, which needs no checking.
    ///
    /// **Not a weaker answer than [`Self::Confirmed`] — a stronger one.** A DOI has to be
    /// verified because the model wrote it. This link was built from the `corpusId` in the search
    /// result the paper came from (`overlay/minime_local/sources.py`), so it does not name the
    /// wrong paper for the same reason a file path does not: nothing composed it.
    FromSearch,
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

    // No `label`. Each verdict used to render one canned line, and the panel now writes a
    // sentence from the verdict *and* what the lookup found — "the citation's own DOI named a
    // different paper; this link is the work it describes" says in one line what two rows of
    // machine phrasing said in two. The distinctions live in the type; the wording belongs where
    // the two facts meet.
}

/// Whether a link is the Semantic Scholar corpus-id form.
///
/// That link comes from the search result itself rather than from the model, so it is the one
/// kind this module has nothing to check — and must not report as unidentified.
pub fn is_corpus_link(link: &str) -> bool {
    link.trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .starts_with("api.semanticscholar.org/CorpusID:")
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

/// The work a bad citation was probably pointing at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repair {
    pub doi: String,
    pub title: String,
}

/// The `(doi, title)` pairs in a Crossref `/works?query.bibliographic=…` response.
pub fn candidates_of(body: &serde_json::Value) -> Vec<(String, String)> {
    let Some(items) = body
        .get("message")
        .and_then(|message| message.get("items"))
        .and_then(|items| items.as_array())
    else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let doi = item.get("DOI")?.as_str()?.trim();
            let title = item.get("title")?.as_array()?.first()?.as_str()?.trim();
            if doi.is_empty() || title.is_empty() {
                return None;
            }
            Some((
                doi.to_string(),
                title.split_whitespace().collect::<Vec<_>>().join(" "),
            ))
        })
        .collect()
}

/// The best candidate that is actually the cited work, or `None`.
///
/// **Held to the same bar as [`judge`], deliberately.** A bibliographic search always returns
/// *something* — Crossref ranks by relevance, not by correctness — so accepting its top hit would
/// replace a wrong DOI with a differently wrong one and present it as the answer. The whole point
/// of this feature is that a researcher can trust what it says, which means it has to be allowed
/// to say nothing.
///
/// The best *scoring* candidate is not used either: the one whose title actually matches the
/// citation is. Those are usually the same and, when they are not, the ranking is what is wrong.
pub fn best_match(citation: &str, candidates: &[(String, String)]) -> Option<Repair> {
    /// How far clear of the runner-up the winner has to be.
    ///
    /// **Because this is a stronger claim than [`judge`] makes.** Verifying a DOI answers "is
    /// this the paper" about one work the citation already named. Repairing *picks* a work and
    /// says "this is the one", so a near-tie is not a weak yes — it is the case where offering
    /// an answer would reproduce the exact bug being fixed, with the app's authority behind it
    /// instead of the model's.
    ///
    /// Measured on the real Plaisted citation: the right paper scored 0.75 and the best wrong
    /// one — *Solanum amayanum: A new wild Peruvian potato species*, which shares "wild",
    /// "potato" and "Solanum" with the model's invented title — scored 0.57. Six points of
    /// threshold is not a margin worth trusting; eighteen points of separation is.
    const MARGIN: f32 = 0.15;

    let mut ranked: Vec<(f32, &String, &String)> = candidates
        .iter()
        .map(|(doi, title)| (overlap(citation, title), doi, title))
        .collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));

    let (score, doi, title) = ranked.first()?;
    if *score < SAME_WORK {
        return None;
    }
    // Two plausible answers is not an answer. Ambiguity is a real result and saying so costs
    // nothing; naming the wrong paper costs a correction in a manuscript.
    if let Some((runner_up, _, _)) = ranked.get(1) {
        if score - runner_up < MARGIN {
            return None;
        }
    }
    Some(Repair {
        doi: (*doi).clone(),
        title: (*title).clone(),
    })
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
    fn the_repair_finds_the_cited_work_or_says_nothing() {
        // The real case: the model wrote the authors, journal, year and pages of Plaisted &
        // Hoopes correctly and produced `…3934` for a DOI. Crossref's bibliographic search
        // returns the right paper among others; the one whose title matches is the answer.
        let citation = "Plaisted, R. L., & Hoopes, R. W. (1989). The past record and future \
                        prospects for the use of exotic potato germplasm. American Potato \
                        Journal, 66, 603–627.";
        let candidates = vec![
            (
                "10.1007/BF02854333".to_string(),
                "Breeding for resistance to potato virus Y".to_string(),
            ),
            (
                "10.1007/BF02853982".to_string(),
                "The past record and future prospects for the use of exotic potato germplasm"
                    .to_string(),
            ),
        ];
        assert_eq!(
            best_match(citation, &candidates),
            Some(Repair {
                doi: "10.1007/BF02853982".to_string(),
                title: "The past record and future prospects for the use of exotic potato \
                        germplasm"
                    .to_string(),
            }),
            "the matching title wins, not the first result"
        );

        // **Nothing rather than the top hit.** A bibliographic search always returns something;
        // accepting it blindly would swap a wrong DOI for a differently wrong one and present
        // that as the answer, which is worse than the bug being fixed.
        let unrelated = vec![
            (
                "10.1/x".to_string(),
                "Algal switching among lichen symbioses".to_string(),
            ),
            (
                "10.1/y".to_string(),
                "Gender topics on potato research and development".to_string(),
            ),
        ];
        assert_eq!(best_match(citation, &unrelated), None);
        assert_eq!(best_match(citation, &[]), None);

        // **A near-tie is not an answer.** These are the top two Crossref actually returned for
        // the model's *invented* wording of that citation ("the use of wild species for the
        // improvement of potato (Solanum tuberosum) varieties"): the right paper at 0.75 and a
        // wild-species paper at 0.57, because the invented title shares "wild", "potato" and
        // "Solanum" with it. Clearing the threshold by six points is not grounds for telling a
        // researcher which paper they meant.
        let invented = "Plaisted, R. L., & Hoopes, R. W. (1989). The past record and future \
                        prospects for the use of wild species for the improvement of potato \
                        (Solanum tuberosum) varieties. American Potato Journal, 66, 603-627.";
        let close = vec![
            (
                "10.1007/bf02853982".to_string(),
                "The past record and future prospects for the use of exotic potato germplasm"
                    .to_string(),
            ),
            (
                "10.1007/bf02853483".to_string(),
                "Solanum amayanum: A new wild Peruvian potato species".to_string(),
            ),
        ];
        let found = best_match(invented, &close).expect("0.75 against 0.57 clears the margin");
        assert_eq!(found.doi, "10.1007/bf02853982");

        // Nudge the runner-up up until the two are close, and it refuses.
        let tied = vec![
            close[0].clone(),
            (
                "10.1/rival".to_string(),
                "The past record and future prospects for the use of exotic potato varieties"
                    .to_string(),
            ),
        ];
        assert_eq!(
            best_match(invented, &tied),
            None,
            "two plausible answers is not an answer"
        );
    }

    #[test]
    fn crossref_search_results_are_read_as_pairs() {
        let body = serde_json::json!({
            "message": {
                "items": [
                    {"DOI": "10.1007/BF02853982",
                     "title": ["The past record and future\n prospects for the use of exotic potato germplasm"],
                     "container-title": ["American Potato Journal"]},
                    // No title: nothing to compare against, so nothing to offer.
                    {"DOI": "10.1/x"},
                    {"title": ["Orphaned, no DOI"]},
                ]
            }
        });
        let found = candidates_of(&body);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "10.1007/BF02853982");
        assert!(found[0].1.starts_with("The past record and future prospects"));
        assert!(!found[0].1.contains('\n'), "the registry's whitespace is normalised");
        assert!(candidates_of(&serde_json::json!({})).is_empty());
    }

    /// The Rust and Python title matchers must reach the same verdict.
    ///
    /// **The same shape as `workspace::project_tests`, and for the same reason.** This rule is now
    /// written twice: `overlay/minime_local/sources.py` uses it to decide which corpus id belongs
    /// to a citation, and this module uses it to decide which registry record does. Both carry the
    /// same noise words, the same 0.6 threshold and the same 0.15 margin, and if they drift the
    /// backend and the client will disagree about which paper a citation names — silently, and in
    /// the one feature built to stop exactly that.
    ///
    /// It cannot be written once, so it is checked instead.
    #[test]
    fn the_rust_and_python_matchers_agree() {
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping: python3 is not on PATH");
            return;
        }
        let overlay = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overlay");

        // (citation, candidate titles) — the real cases from §119 and §120, plus the ties.
        let cases: [(&str, &[&str]); 5] = [
            (
                "Plaisted, R. L., & Hoopes, R. W. (1989). The past record and future prospects \
                 for the use of wild species for the improvement of potato (Solanum tuberosum) \
                 varieties. American Potato Journal, 66, 603-627.",
                &[
                    "The past record and future prospects for the use of exotic potato germplasm",
                    "Solanum amayanum: A new wild Peruvian potato species",
                ],
            ),
            (
                "Hijmans, R.J., & Spooner, D.M. (2001). Geographic distribution of wild potato \
                 species. American Journal of Botany, 88(11), 2101-2112.",
                &["Algal switching among lichen symbioses"],
            ),
            (
                "Hijmans, R.J., & Spooner, D.M. (2001). Geographic distribution of wild potato \
                 species. American Journal of Botany, 88(11), 2101-2112.",
                &["Geographic distribution of wild potato species"],
            ),
            // A tie: both plausible, so both sides must decline.
            (
                "Smith (2020). The potato crop handbook.",
                &["The potato crop handbook", "The potato crop handbook II"],
            ),
            ("Nothing recognisable here.", &["Some unrelated title"]),
        ];

        let source = std::fs::read_to_string(overlay.join("minime_local/sources.py"))
            .expect("the overlay is beside the crate");
        // Only the pure functions, so importing does not pull in contextvars-backed state.
        let start = source.find("_NOISE = {").expect("the noise set");
        let end = source.find("def _papers()").expect("the next function");
        let script = format!(
            "import json,re,sys\n{}\n\
             cit, titles = sys.argv[1], json.loads(sys.argv[2])\n\
             have = set(_significant(cit))\n\
             ranked = []\n\
             for t in titles:\n\
             \x20   want = _significant(t)\n\
             \x20   if want: ranked.append((sum(w in have for w in want)/len(want), t))\n\
             ranked.sort(reverse=True)\n\
             ok = bool(ranked) and ranked[0][0] >= 0.6 and (len(ranked) < 2 or ranked[0][0]-ranked[1][0] >= 0.15)\n\
             print(json.dumps(ranked[0][1] if ok else None))",
            &source[start..end]
        );

        for (citation, titles) in cases {
            let candidates: Vec<(String, String)> = titles
                .iter()
                .enumerate()
                .map(|(at, title)| (format!("10.1/{at}"), title.to_string()))
                .collect();
            let ours = best_match(citation, &candidates).map(|repair| repair.title);

            let out = std::process::Command::new("python3")
                .arg("-c")
                .arg(&script)
                .arg(citation)
                .arg(serde_json::to_string(titles).expect("json"))
                .output()
                .expect("python3 runs");
            assert!(
                out.status.success(),
                "python failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let theirs: Option<String> =
                serde_json::from_slice(&out.stdout).expect("python printed json");

            assert_eq!(ours, theirs, "disagreed on {citation:?} against {titles:?}");
        }
    }

    #[test]
    fn a_corpus_link_is_recognised_and_never_treated_as_a_doi() {
        // The form `overlay/minime_local/sources.py` writes, and the one `_paper_ref` established
        // resolves correctly.
        for link in [
            "https://api.semanticscholar.org/CorpusID:45447591",
            "http://api.semanticscholar.org/CorpusID:237412855",
            " https://api.semanticscholar.org/CorpusID:1 ",
        ] {
            assert!(is_corpus_link(link), "{link}");
            // And it carries no DOI, so it must never be sent to Crossref — which would 404 and
            // report a correctly identified paper as unregistered.
            assert_eq!(doi_in(link), None, "{link}");
        }
        assert!(!is_corpus_link("https://doi.org/10.1007/BF02853982"));
        assert!(!is_corpus_link("https://www.semanticscholar.org/paper/abc123"));
        assert!(!is_corpus_link(""));
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
