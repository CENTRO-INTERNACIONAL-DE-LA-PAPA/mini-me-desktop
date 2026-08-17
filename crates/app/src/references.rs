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

/// Where a reference came from — a different question from whether anything is wrong with it.
///
/// # Why this is not the same as [`Verdict`]
///
/// The panel's rule has been *"only when something is wrong"*, on the sound argument that a line
/// under every reference saying it checked out is fourteen lines of reassurance nobody reads.
/// But that rule answers **is this broken**, and it made silence carry a second meaning it had
/// not earned: *nothing is wrong here* and *this came from a search* and *the model wrote this
/// down from memory and nothing has confirmed it* all rendered identically.
///
/// Those are not the same fact, and for this institution the difference is the point. Barrera et
/// al. (2016) came back real, relevant, and from a journal Semantic Scholar indexes poorly —
/// which describes a great deal of CIP's own literature. A reference like that is not an error to
/// be flagged; it is a citation a **subject-matter expert has to check by hand**, and the
/// researcher cannot know which ones those are if they look exactly like the verified ones.
/// Org policy asks for exactly this and in these words: *validate AI-generated content with
/// subject matter experts*, and *disclose when generative AI has been used*.
///
/// # What each one means
///
/// Named for **what was done**, never for what is true — the same rule the rest of this module
/// follows. `Unconfirmed` does not mean invented; most of these are real papers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The paper arrived inside a search result, and its link was built from the `corpusId` in
    /// that result. Nothing here was composed by a model.
    Search,
    /// A model wrote an identifier down and a registry agrees it names this paper — either its
    /// own DOI resolved to the cited title, or Crossref matched the citation to a real work.
    Registry,
    /// Nothing has confirmed this one. It may be perfectly correct; the point is that saying so
    /// would take a person.
    Unconfirmed,
    /// The check has not come back yet. Distinct from [`Self::Unconfirmed`] because reporting a
    /// reference as unchecked while its lookup is in flight is the §(references) bug — a
    /// correctly cited Magurran 1988 told it matched nothing, mid-request.
    Pending,
}

impl Origin {
    /// The phrase a row carries, in the researcher's terms.
    ///
    /// `None` for the two that need no words: a reference nothing is wrong with does not want a
    /// badge, and one still resolving already has the section's own "checking…" line.
    pub fn note(self) -> Option<&'static str> {
        match self {
            Origin::Unconfirmed => Some("unverified — from the model, not from a search"),
            Origin::Search | Origin::Registry | Origin::Pending => None,
        }
    }

    /// Whether this is one a person still has to look at.
    pub fn needs_a_human(self) -> bool {
        matches!(self, Origin::Unconfirmed)
    }
}

/// Where a reference came from, given what the check found and whether the registry matched it.
///
/// Both arguments, because neither settles it alone: a citation whose own DOI named the wrong
/// paper is [`Origin::Registry`] when Crossref then found the right one and [`Origin::Unconfirmed`]
/// when it did not, and the [`Verdict`] is `Mismatch` either way.
///
/// `matched_in_registry` is `None` while the repair lookup is in flight — the same three-state
/// distinction the panel keeps unflattened, for the same reason.
pub fn origin(verdict: Option<&Verdict>, matched_in_registry: Option<bool>) -> Origin {
    match verdict {
        None => Origin::Pending,
        Some(Verdict::FromSearch) => Origin::Search,
        Some(Verdict::Confirmed) => Origin::Registry,
        // A registry match is what rescues these, and until the lookup returns nothing is settled.
        Some(Verdict::Mismatch { .. } | Verdict::Unregistered) => match matched_in_registry {
            Some(true) => Origin::Registry,
            Some(false) => Origin::Unconfirmed,
            None => Origin::Pending,
        },
        // No identifier to check, so only a title match could confirm it.
        Some(Verdict::NoIdentifier) => match matched_in_registry {
            Some(true) => Origin::Registry,
            Some(false) => Origin::Unconfirmed,
            None => Origin::Pending,
        },
        // **Unconfirmed, not pending.** The check failed and is not coming back on its own, and a
        // reference stuck on "checking…" forever reads as verified to anybody who looks away and
        // returns. What is unknown has to say so.
        Some(Verdict::Unreachable { .. }) => Origin::Unconfirmed,
    }
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

    /// The Python this machine has, by the name that works here.
    ///
    /// **Two things Windows does differently, and both cost a red release build (§214).**
    ///
    /// `python3` is not the name there: the launcher is `py` and the interpreter is `python`.
    /// Tried in order so a machine with any of them runs these tests rather than skipping every
    /// one — which is the worse failure, because it is green.
    ///
    /// The **program name**, not a `Command`: a `Command` accumulates its arguments, and two of
    /// these tests spawn inside a loop. Handing the same one round a loop reruns the first
    /// iteration's arguments with the second iteration's expectation, which fails while pointing
    /// at the wrong record. Caught locally; it would have been a genuinely confusing red build.
    fn python() -> Option<&'static str> {
        ["python3", "python", "py"].into_iter().find(|name| {
            std::process::Command::new(name)
                .arg("--version")
                .output()
                .is_ok_and(|out| out.status.success())
        })
    }

    /// A fresh interpreter, told to speak UTF-8.
    ///
    /// `PYTHONIOENCODING` is the load-bearing part. Python picks its stdout encoding from the
    /// console code page, which on a Windows runner is cp1252 — so an en-dash written by the
    /// overlay arrives as the single byte `0x96`, and reading it back as UTF-8 gives `�`. The test
    /// then compares `603�627` against `603–627` and fails on a citation the overlay had formatted
    /// perfectly. Nothing about the product: in a real run this text crosses HTTP as JSON, which is
    /// UTF-8 by definition, and the overlay only ever runs inside WSL.
    fn interpreter(program: &str) -> std::process::Command {
        let mut command = std::process::Command::new(program);
        command.env("PYTHONIOENCODING", "utf-8");
        command
    }

    #[test]
    fn where_a_reference_came_from_is_a_separate_question_from_whether_it_is_broken() {
        use Origin::*;

        // The two that need nobody: one built from a search result, one whose identifier a
        // registry confirmed. Neither carries a note, which is the existing rule and stays.
        assert_eq!(origin(Some(&Verdict::FromSearch), None), Search);
        assert_eq!(origin(Some(&Verdict::Confirmed), Some(false)), Registry);
        assert!(Search.note().is_none() && Registry.note().is_none());
        assert!(!Search.needs_a_human() && !Registry.needs_a_human());

        // Barrera et al. (2016): real, relevant, from a journal Semantic Scholar indexes poorly.
        // No identifier, nothing in Crossref — not an error, but a citation only a person can
        // settle, and until §185 it rendered exactly like the two above.
        assert_eq!(origin(Some(&Verdict::NoIdentifier), Some(false)), Unconfirmed);
        assert!(Unconfirmed.needs_a_human());
        assert!(Unconfirmed.note().is_some_and(|n| n.contains("not from a search")));

        // A wrong DOI is rescued by a registry match and unconfirmed without one — and the
        // verdict is `Mismatch` either way, which is why one argument could not decide this.
        let wrong = Verdict::Mismatch {
            found: "a different paper".into(),
        };
        assert_eq!(origin(Some(&wrong), Some(true)), Registry);
        assert_eq!(origin(Some(&wrong), Some(false)), Unconfirmed);
        assert_eq!(origin(Some(&Verdict::Unregistered), Some(true)), Registry);

        // **Mid-flight is its own answer.** Reporting a reference as unchecked while its lookup
        // is still running is the bug that told a correctly cited Magurran 1988 it matched
        // nothing, and `None` for the repair is exactly that state.
        assert_eq!(origin(None, None), Pending);
        assert_eq!(origin(Some(&Verdict::NoIdentifier), None), Pending);
        assert_eq!(origin(Some(&wrong), None), Pending);
        assert!(Pending.note().is_none(), "the section already says 'checking…'");
        assert!(!Pending.needs_a_human(), "not yet — it may still come back verified");

        // But a check that *failed* is not pending: nothing is going to come back, and a row
        // stuck on "checking…" reads as verified to anyone who looks away and returns.
        let offline = Verdict::Unreachable {
            why: "offline".into(),
        };
        assert_eq!(origin(Some(&offline), None), Unconfirmed);
        assert_eq!(origin(Some(&offline), Some(true)), Unconfirmed);
    }

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
        let Some(python) = python() else {
            eprintln!("skipping: no Python on PATH");
            return;
        };
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

            let out = interpreter(python)
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

    /// The overlay builds APA references in code. This checks it against the real records the
    /// model got wrong.
    ///
    /// Driven from Rust for the same reason `the_rust_and_python_matchers_agree` is: this repo has
    /// no Python harness, and a rule that only runs in production is a rule nobody checks.
    #[test]
    fn the_overlay_builds_a_citation_the_model_could_not() {
        let Some(python) = python() else {
            eprintln!("skipping: no Python on PATH");
            return;
        };
        let overlay = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overlay");

        // Real Semantic Scholar records, verbatim. The first two are the papers whose DOIs the
        // model invented in a live run (§119, §120): it wrote BF02853934 for Plaisted, whose real
        // DOI is BF02853982, and 3558457 for Hijmans, whose real DOI is 3558435 — the wrong one
        // belonging to a study of lichen symbioses.
        let cases: [(&str, &str); 5] = [
            (
                r#"{"authors":[{"name":"R. Plaisted"},{"name":"R. Hoopes"}],"year":1989,
                    "title":"The past record and future prospects for the use of exotic potato germplasm",
                    "journal":{"name":"American Potato Journal","volume":"66","pages":"603-627"},
                    "externalIds":{"DOI":"10.1007/BF02853982"}}"#,
                "Plaisted, R., & Hoopes, R. (1989). The past record and future prospects for the \
                 use of exotic potato germplasm. American Potato Journal, 66, 603–627. \
                 https://doi.org/10.1007/BF02853982",
            ),
            (
                // `"88 11"` is how S2 packs volume and issue — it must render as 88(11).
                r#"{"authors":[{"name":"R. Hijmans"},{"name":"D. Spooner"}],"year":2001,
                    "title":"Geographic distribution of wild potato species",
                    "journal":{"name":"American Journal of Botany","volume":"88 11","pages":"2101-2112"},
                    "externalIds":{"DOI":"10.2307/3558435"}}"#,
                "Hijmans, R., & Spooner, D. (2001). Geographic distribution of wild potato \
                 species. American Journal of Botany, 88(11), 2101–2112. \
                 https://doi.org/10.2307/3558435",
            ),
            (
                // A surname particle. CIP authors have these, and splitting one wrongly is a
                // misattribution rather than a formatting slip.
                r#"{"authors":[{"name":"M. del R. Herrera"},{"name":"Jonathan D. G. Jones"}],
                    "year":2004,"title":"A paper",
                    "journal":{"name":"Theoretical and Applied Genetics"},
                    "externalIds":{}}"#,
                "Herrera, M. del R., & Jones, J. D. G. (2004). A paper. Theoretical and Applied \
                 Genetics.",
            ),
            (
                // Nothing but a title: an incomplete reference a person can finish, rather than
                // a complete one they cannot check.
                r#"{"title":"An orphan record"}"#,
                "(n.d.). An orphan record.",
            ),
            (
                // Semantic Scholar indents its `pages` field across newlines, which reached the
                // rendered reference as `65,\n          1-8\n        .` until `_clean` collapsed
                // it. Found by comparing seventeen live records against Crossref, where it was
                // the only thing that looked like a disagreement and was not one.
                r#"{"authors":[{"name":"M. Alquraishi"}],"year":2021,
                    "title":"Machine learning in protein structure prediction",
                    "journal":{"name":"Current opinion in chemical biology","volume":"65",
                               "pages":"\n          1-8\n        "},
                    "externalIds":{"DOI":"10.1016/j.cbpa.2021.04.005"}}"#,
                "Alquraishi, M. (2021). Machine learning in protein structure prediction. \
                 Current opinion in chemical biology, 65, 1–8. \
                 https://doi.org/10.1016/j.cbpa.2021.04.005",
            ),
        ];

        for (record, expected) in cases {
            let script = format!(
                "import json,sys\nsys.path.insert(0,{overlay:?})\n\
                 from minime_local import citations\n\
                 print(citations.apa(json.loads(sys.argv[1])))",
                overlay = overlay.to_string_lossy()
            );
            let out = interpreter(python)
                .arg("-c")
                .arg(&script)
                .arg(record)
                .output()
                .expect("python3 runs");
            assert!(
                out.status.success(),
                "python failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let got = String::from_utf8_lossy(&out.stdout).trim().to_string();
            // The expected strings are line-continued in this source; compare on collapsed space.
            let squash = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
            assert_eq!(squash(&got), squash(expected), "record: {record}");
        }
    }

    /// The overlay's tool wrapper must actually *run*, not merely parse.
    ///
    /// **This test exists because of a shipped crash.** `install_mcp` built its wrapper with
    /// `_tool=name`, where `name` belonged to upstream's loop and not to ours. Python evaluates a
    /// default argument when the `def` executes, so it raised `NameError: name 'name' is not
    /// defined` the moment the tool list was wrapped — and every turn in the app failed with
    /// "An internal error occurred". It went out because it was checked with `ast.parse`, which
    /// proves a file is syntactically valid and never that a line of it runs (docs §128).
    ///
    /// Driven from Rust because this repository has no Python harness, and an overlay that only
    /// executes in production is one nobody tests.
    #[test]
    fn the_overlays_tool_wrapper_runs() {
        let Some(python) = python() else {
            eprintln!("skipping: no Python on PATH");
            return;
        };
        let overlay = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overlay");
        // Upstream's wrapper around ours, with the tool call in a child task — the arrangement
        // the backend actually uses, and the one where a ContextVar store silently failed (§123).
        let script = format!(
            r#"
import sys, types, asyncio, json
sys.path.insert(0, {overlay:?})
from minime_local import sources

mod = types.ModuleType("backend.mcp_tools")
def _make_mcp_tools_resilient(tools):
    for t in tools:
        inner = t.coroutine
        async def capped(*a, _i=inner, **k):
            await _i(*a, **k)
            return "TRUNCATED"
        t.coroutine = capped
    return tools
mod._make_mcp_tools_resilient = _make_mcp_tools_resilient

payload = [{{"type": "text", "text": json.dumps(
    {{"data": [{{"paper": {{"corpusId": "237744014", "title": "A recorded paper title"}}}}]}})}}]

class Tool:
    name = "snippet_search"
    def __init__(self):
        async def coro(**kw):
            return payload
        self.coroutine = coro

sources.install_mcp(mod)
tools = mod._make_mcp_tools_resilient([Tool()])
async def main():
    await asyncio.create_task(tools[0].coroutine())
    print(len(sources._papers()))
asyncio.run(main())
"#,
            overlay = overlay.to_string_lossy()
        );
        let out = interpreter(python)
            .arg("-c")
            .arg(&script)
            .output()
            .expect("python3 runs");
        assert!(
            out.status.success(),
            "the overlay's wrapper raised:
{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "1",
            "the recorder saw the raw result before upstream truncated it"
        );
    }

    /// `find_papers` is recorded too, and keeps the link the record gave it.
    ///
    /// **This one exists because of four days spent reading the wrong half of the system.** The
    /// only evidence in the backend log was `0 of N sources carry the corpus id`, which was taken
    /// as "the subagent invented its citations again". It says exactly the same thing when the
    /// subagent did everything right: `find_papers` is not part of the MCP bundle, so it never
    /// passed through the wrapper that records papers, and a perfect run recorded nothing and
    /// printed zero.
    ///
    /// Asserts the link is passed through rather than rebuilt: `backend/citations.py` prefers the
    /// DOI from the publisher's record, and a corpus id reconstructed here would be worse.
    #[test]
    fn the_overlay_records_the_cli_search_as_well_as_the_mcp_one() {
        let Some(python) = python() else {
            eprintln!("skipping: no Python on PATH");
            return;
        };
        let overlay = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overlay");
        let script = format!(
            r#"
import sys, types, asyncio, json
sys.path.insert(0, {overlay:?})
from minime_local import sources

mod = types.ModuleType("backend.paper_tools")
found = json.dumps({{"query": "q", "count": 1, "papers": [{{
    "citation": "Sorensen, T. (1948). A method of establishing groups of equal amplitude.",
    "link": "https://api.semanticscholar.org/DOI:10.1234/abcd",
    "title": "A method of establishing groups of equal amplitude in plant sociology"}}]}})

class Tool:
    name = "find_papers"
    def __init__(self):
        async def coro(query, limit=10):
            return found
        self.coroutine = coro

mod.find_papers = Tool()
sources.install_papers(mod)
async def main():
    # In a child task, like every other tool call the backend makes (§123).
    await asyncio.create_task(mod.find_papers.coroutine("beta diversity"))
    print(sources.link_for(
        "Sorensen, T. (1948). A method of establishing groups of equal "
        "amplitude in plant sociology. Biologiske Skrifter."))
asyncio.run(main())
"#,
            overlay = overlay.to_string_lossy()
        );
        let out = interpreter(python)
            .arg("-c")
            .arg(&script)
            .output()
            .expect("python3 runs");
        assert!(
            out.status.success(),
            "the overlay's find_papers wrapper raised:
{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "https://api.semanticscholar.org/DOI:10.1234/abcd",
            "the recorded link is the one the record supplied, not a rebuilt corpus id"
        );
    }

    /// A failed command tells the model which directory it ran in.
    ///
    /// **Two blind attempts, on a real run.** A background worker wrote `potato_late_blight.csv`
    /// into its workspace, then shelled out to plot it with
    /// `pd.read_csv('/data/potato_late_blight.csv')` — exit 1 — and retried with
    /// `/home/piero_linux/Mini-Me/...` — exit 1. Neither exists. Commands already run *with the
    /// workspace as their working directory*, so the bare filename would have worked first time.
    ///
    /// The path was never a secret; `aresolve` announces it to the desktop status line. The one
    /// participant who needed it could not see it, and the error it got back named the directory it
    /// had invented rather than the one it had. The turn then reported that plots had been saved.
    #[test]
    fn a_failed_command_tells_the_model_where_it_ran() {
        let Some(python) = python() else {
            eprintln!("skipping: no Python on PATH");
            return;
        };
        let overlay = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overlay");
        // The helpers are lifted out of the module verbatim rather than imported, because
        // importing `minime_local.workspace` drags in deepagents and langgraph.
        let script = format!(
            r#"
import logging, pathlib
src = pathlib.Path({overlay:?} + "/minime_local/workspace.py").read_text()
ns = {{"Any": object, "logger": logging.getLogger("t")}}
exec(src[src.index("_CWD_NOTE ="):src.index("def _log_failure")], ns)
say = ns["_say_where_it_ran"]

class R:
    def __init__(self, code, out): self.exit_code, self.output = code, out

failed = R(1, "FileNotFoundError: '/data/x.csv'")
say(failed, "/work/thread-1")
assert "/work/thread-1" in failed.output, failed.output
assert "relative" in failed.output, failed.output

# A working command stays quiet: a line appended to every execute is a line the model
# learns to skip, which is how the corpus-id diagnostic stopped being read.
worked = R(0, "done")
say(worked, "/work/thread-1")
assert worked.output == "done", worked.output

# Not repeated when the output already names the directory.
knew = R(1, "cannot open /work/thread-1/x.csv")
say(knew, "/work/thread-1")
assert knew.output.count("/work/thread-1") == 1, knew.output

# Both response shapes the sandbox protocol returns.
mapping = {{"exit_code": 2, "output": "boom"}}
say(mapping, "/work/thread-1")
assert "/work/thread-1" in mapping["output"], mapping

# A shape it cannot annotate costs a hint, never the command.
say(None, "/work/thread-1")
say(object(), "/work/thread-1")
print("ok")
"#,
            overlay = overlay.to_string_lossy()
        );
        let out = interpreter(python)
            .arg("-c")
            .arg(&script)
            .output()
            .expect("python3 runs");
        assert!(
            out.status.success(),
            "the overlay's cwd hint is wrong:
{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A background worker writes into the conversation's folder, not its own.
    ///
    /// **Thirteen files in a directory nobody opens.** A worker produced six plots and seven
    /// tables correctly, and they landed under its *task* id while the coordinator reported them
    /// under the *conversation* id and the Files panel showed neither. The researcher was told the
    /// plots were saved, opened the folder, and found one stale CSV.
    ///
    /// The pin came from `configurable["thread_id"]` alone, and on that run it was absent — while
    /// `model_config` and `__workspace_project__`, read from the same `configurable` two lines
    /// above, arrived intact. So the worker inherited the conversation's *project folder* and not
    /// its thread, which is why the files were one directory sideways rather than lost.
    #[test]
    fn background_work_is_pinned_to_the_conversation_from_whichever_key_has_it() {
        let Some(python) = python() else {
            eprintln!("skipping: no Python on PATH");
            return;
        };
        let overlay = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overlay");
        let script = format!(
            r#"
import pathlib, sys, types
src = pathlib.Path({overlay:?} + "/minime_local/async_agents.py").read_text()
stub = types.ModuleType("minime_local.workspace")
stub.WORKSPACE_THREAD_KEY = "__workspace_thread__"
sys.modules["minime_local"] = types.ModuleType("minime_local")
sys.modules["minime_local.workspace"] = stub
ns = {{}}
exec(src[src.index("def _conversation_thread"):src.index("def _forwarded_config")], ns)
thread = ns["_conversation_thread"]

# An existing pin wins, so a worker started by a worker keeps the original conversation
# rather than adopting its parent's.
assert thread({{}}, {{"__workspace_thread__": "conv", "thread_id": "parent"}})[0] == "conv"

# Each remaining source, in turn. `configurable.thread_id` is where LangGraph documents it
# (`pregel/main.py` reads `saved.config[CONF]["thread_id"]`) and is tried first.
assert thread({{}}, {{"thread_id": "conv"}}) == ("conv", "configurable.thread_id")
assert thread({{"metadata": {{"thread_id": "conv"}}}}, {{}}) == ("conv", "metadata.thread_id")
assert thread({{}}, {{"__thread_id__": "conv"}})[0] == "conv"

# Whitespace is not an id: a blank must fall through, not win and produce a directory
# named after nothing.
assert thread({{"metadata": {{"thread_id": "conv"}}}}, {{"thread_id": "   "}})[0] == "conv"

# And when nothing has it, that is reported rather than guessed at. The caller logs this,
# because an unpinned worker fails *silently* — it fills a real directory correctly and
# reports paths under a different one.
assert thread({{}}, {{}}) == ("", "nothing")
print("ok")
"#,
            overlay = overlay.to_string_lossy()
        );
        let out = interpreter(python)
            .arg("-c")
            .arg(&script)
            .output()
            .expect("python3 runs");
        assert!(
            out.status.success(),
            "the workspace pin is wrong:
{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A background worker's folder sits **inside** the conversation's, not beside it.
    ///
    /// > *"the idea its to somehow view it in the app not as a diferent folder outside the
    /// > conversation folder"*
    ///
    /// Three attempts moved these files between sibling directories and none answered that.
    /// Nesting does, with no client change: `workspace::outputs` already descends through named
    /// subfolders and shows the relative path (§143), so the worker's files appear in the
    /// conversation's Outputs panel labelled by the run that made them — which writing straight
    /// into the conversation's folder would have destroyed by mixing every worker together.
    #[test]
    fn a_background_workers_folder_is_nested_inside_the_conversations() {
        let Some(python) = python() else {
            eprintln!("skipping: no Python on PATH");
            return;
        };
        let overlay = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overlay");
        // The layout is lifted out of `LocalSandbox.__init__` verbatim — importing the module
        // drags in deepagents and langgraph.
        let script = format!(
            r#"
import pathlib
src = pathlib.Path({overlay:?} + "/minime_local/workspace.py").read_text()

# Tied to the real code: if the nesting line is edited away, this fails rather than passing
# against a copy that no longer matches.
assert "parts.append(thread_id)" in src, "the worker no longer nests"
assert "root.joinpath(" in src, "the work dir is no longer composed from parts"

def layout(root, project, pinned, own):
    parts = [pinned] + ([own] if own and own != pinned else [])
    # `as_posix`, not `str`: the rule under test is which folder nests inside which, and `str`
    # of a WindowsPath spells the same layout with backslashes. The overlay itself only ever
    # runs inside WSL, so its own separator is never in question (§214).
    return pathlib.Path(root).joinpath(*([project] if project else []), *parts).as_posix()

# A worker nests inside the conversation; the conversation itself does not move.
assert layout("/w", "proj", "A", "B") == "/w/proj/A/B", layout("/w", "proj", "A", "B")
assert layout("/w", "proj", "A", "A") == "/w/proj/A"
assert layout("/w", "", "A", "B") == "/w/A/B"
assert layout("/w", "", "A", "A") == "/w/A"

# An unpinned worker still gets its own folder rather than none: a failed pin must cost
# discoverability, never the files (§150).
assert layout("/w", "", "B", "B") == "/w/B"

# **The pin is remembered per thread.** A single background run built its sandbox twice — once
# where `get_config()` carried the pin and once where it did not — producing two directories
# for one task: the nested one, empty, and a sibling holding every file (§151). The same shape
# as §123, where a ContextVar store did not survive a task boundary.
import logging
body = src[src.index("_PINNED_BY_THREAD: dict[str, str] = {{}}"):src.index("logger = logging.getLogger(__name__)")]
cfg = {{"value": {{}}}}
ns = {{"logger": logging.getLogger("t"), "_configurable": lambda: cfg["value"],
      "WORKSPACE_THREAD_KEY": "__workspace_thread__"}}
exec(body, ns)
thread = ns["workspace_thread"]

cfg["value"] = {{}}
assert thread("A") == "A", "a coordinator must never nest"
cfg["value"] = {{"__workspace_thread__": "A"}}
assert thread("B") == "A"
cfg["value"] = {{}}
assert thread("B") == "A", "the pin was lost at the second construction site"
cfg["value"] = {{"__workspace_thread__": "C"}}
assert thread("D") == "C"
cfg["value"] = {{}}
assert thread("D") == "C" and thread("A") == "A", "one task belongs to one conversation"
print("ok")
"#,
            overlay = overlay.to_string_lossy()
        );
        let out = interpreter(python)
            .arg("-c")
            .arg(&script)
            .output()
            .expect("python3 runs");
        assert!(
            out.status.success(),
            "the workspace layout is wrong:
{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// `execute` is told to keep its output where the researcher can see it.
    ///
    /// **Sixteen real files went to WSL's global `/tmp`** — a 46 KB dataset, seven summary CSVs,
    /// seven figures — and the Outputs panel was empty (§160). The coordinator had asked for
    /// relative paths and lost to deepagents' own execute description, which says *"maintain your
    /// current working directory … by using absolute paths"*. Sound advice in a container the
    /// agent owns; here `virtual_mode=False` means an absolute path is the researcher's real
    /// filesystem.
    ///
    /// This asserts the rewrite applies **and that it is honest when it cannot**: the replacement
    /// targets an exact sentence, so an upstream rewording must be reported rather than silently
    /// producing a description that still argues for absolute paths.
    #[test]
    fn execute_is_told_to_keep_its_output_in_the_workspace() {
        let Some(python) = python() else {
            eprintln!("skipping: no Python on PATH");
            return;
        };
        let overlay = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overlay");
        let script = format!(
            r#"
import logging, sys, types
sys.path.insert(0, {overlay:?})
from minime_local import execute_rule

logs = []
execute_rule.log = type("L", (), {{"warning": lambda self, m, *a: logs.append(m % a if a else m)}})()

# Upstream's text, with the sentence this exists to remove.
upstream = types.SimpleNamespace(EXECUTE_TOOL_DESCRIPTION=(
    "Executes a shell command.\n"
    "  - When issuing multiple commands, use the ';' or '&&' operator\n"
    "  - Try to maintain your current working directory throughout the session by using "
    "absolute paths and avoiding usage of cd\n"))
execute_rule.install(upstream)
out = upstream.EXECUTE_TOOL_DESCRIPTION

assert "using absolute paths and avoiding" not in out, "the advice that caused the escape survived"
assert "already this conversation" in out, "nothing told it where it is"
assert "/tmp" in out, "the rule must name the directory the model actually guessed"
# Upstream's unrelated guidance is left exactly as written — this replaces one sentence, not
# a document somebody else maintains.
assert "the ';' or '&&' operator" in out
# Reading elsewhere stays allowed: a researcher attaches datasets from anywhere (§28), and a
# rule that forbade absolute reads would fix outputs by breaking inputs.
assert "Reading an absolute path is fine" in out
assert not any("no longer contains" in line for line in logs), logs

# Upstream reworded: the rule still ships, and the log says the contradiction may be back.
logs.clear()
moved = types.SimpleNamespace(EXECUTE_TOOL_DESCRIPTION="Prefer fully-qualified paths at all times.")
execute_rule.install(moved)
assert "Where your output goes" in moved.EXECUTE_TOOL_DESCRIPTION
assert any("no longer contains" in line for line in logs), logs

# The constant is gone entirely: say so, change nothing, and never raise — an exception here
# would cost the whole agent to prevent files landing in the wrong folder.
logs.clear()
execute_rule.install(types.SimpleNamespace())
assert any("no EXECUTE_TOOL_DESCRIPTION" in line for line in logs), logs
print("ok")
"#,
            overlay = overlay.to_string_lossy()
        );
        let out = interpreter(python)
            .arg("-c")
            .arg(&script)
            .output()
            .expect("python3 runs");
        assert!(
            out.status.success(),
            "the execute rule is wrong:
{}",
            String::from_utf8_lossy(&out.stderr)
        );
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
