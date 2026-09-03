//! Naming a specialist instead of hoping the coordinator picks it.
//!
//! Requested as `/eda-subagent make an EDA of data.csv`, `/research-paper search this topic`,
//! `/report-write write a report from these papers` (docs §55). Today the coordinator decides
//! what to delegate; this lets the researcher say it outright.
//!
//! # What this is, precisely
//!
//! It is **not** a way to bypass the coordinator, and pretending otherwise would be the easy
//! mistake. `start_async_task` and `task` are *tools the agent holds* — there is no endpoint on
//! the server that runs one subagent. So a `/subagent` command composes a turn that names the
//! specialist and says what to do with it, and the coordinator delegates. Three consequences,
//! all of them good:
//!
//! - the approval gate still applies, because nothing went around it (§19, §41, and §55's
//!   fourth point, which asked for exactly this);
//! - background dispatch is the same mechanism with a different instruction, not a second code
//!   path;
//! - it works against the pinned backend with no upstream change at all.
//!
//! What the app *does* own is the name: it is checked against the registry the backend overlay
//! writes (`overlay/minime_local/registry.py`, read by [`crate::workspace::subagents`]) before
//! anything is sent. §55 left this as the open question and answered it — "failing loudly at
//! send is right; silently sending `/eda-subagent …` as prose is how someone waits ten minutes
//! for a turn that was never delegated."
//!
//! # Why matching looks at the description too
//!
//! The names the request imagined are not the names the backend uses. `/eda-subagent` is
//! `exploratory_data_analysis`, `/research-paper` is `academic_researcher`, `/report-write` is
//! `report_writer`. Scoring the name alone finds the first and third by subsequence and misses
//! anything phrased the way a person thinks about it, so each specialist is scored on its name
//! *and* its own description, whichever reads better.

use crate::workspace::Subagent;

/// How a specialist reads in the UI: its name formatted for reading, and the colour its dot is
/// tinted — `academic_researcher` becomes `("Academic Researcher", 0xF47920)`.
///
/// Colours are placeholders — nothing here claims meaning beyond letting the same specialist
/// read as the same dot everywhere it appears (the indicator above the composer, its own row in
/// the menu). Named explicitly rather than assigned in registry order, because that order is the
/// backend's and can be rebuilt any time the overlay reassembles the coordinator (§55) — a
/// colour tied to position would then reshuffle across every specialist for no reason visible to
/// whoever is looking at it. A specialist this table doesn't yet name — one the backend has added
/// since this was last written — still gets its name formatted and falls back to grey, which is
/// not a broken lookup, just one not yet given its own colour.
pub fn display(name: &str) -> (String, u32) {
    let display_name = match name {
        "academic_researcher" => "Academic Researcher".into(),
        "dataverse_explorer" => "Dataverse Explorer".into(),
        "data_cleaning" => "Data Cleaning".into(),
        "exploratory_data_analysis" => "Exploratory Data Analysis".into(),
        "diagnostic_analytics" => "Diagnostic Analytics".into(),
        "predictive_analytics" => "Predictive Analytics".into(),
        "report_writer" => "Report Writer".into(),
        "hypothesis_generator" => "Hypothesis Generator".into(),
        "pdf_librarian" => "PDF Librarian".into(),
        "data_voyager" => "Data Voyager".into(),
        "autodiscovery" => "AutoDiscovery".into(),
        "research_planner" => "Research Planner".into(),
        // A specialist this table doesn't yet name — added since this was last written — is
        // still shown as something, rather than failing to compile or panicking on it.
        _ => name.to_string(),
    };
    let colour = match name {
        "academic_researcher" => 0xF47920,
        "dataverse_explorer" => 0x20ADF4,
        "data_cleaning" => 0xF42091,
        "exploratory_data_analysis" => 0x8B5CF6,
        "diagnostic_analytics" => 0x22C55E,
        "predictive_analytics" => 0xEAB308,
        "report_writer" => 0x06B6D4,
        "hypothesis_generator" => 0xEF4444,
        "pdf_librarian" => 0x14B8A6,
        "data_voyager" => 0xEC4899,
        "autodiscovery" => 0xF97316,
        "research_planner" => 0x3B82F6,
        _ => crate::theme::text_faint(),
    };
    (display_name, colour)
}

/// A `/name …` typed into the composer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// What was typed after the slash and before the first space — possibly a partial name, or
    /// a wrong one. Never assumed to be real.
    pub name: String,
    /// Everything after it. May be empty while the name is still being typed.
    pub prompt: String,
}

/// The characters a specialist name is made of.
///
/// Every name the backend registers is snake_case ASCII — `academic_researcher`,
/// `pdf_librarian`, `data_voyager`. Hyphens are allowed because refusing them would turn a near
/// miss into "this is not a command at all", and the picker's "did you mean" is the better answer.
fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// Whether what follows the slash could be a specialist name at all.
///
/// **A path is not a command.** Attaching a file puts its absolute path in the composer, and on
/// Windows that is a WSL path — `/mnt/c/Users/…/Graph-neural-networks.pdf`. It begins with a slash
/// and contains no whitespace, so the parser read the entire path as a name and the researcher was
/// told *no specialist called "mnt/c/Users/LENOVO/Downloads/Graph-neural-networks.pdf"* for a turn
/// that never mentioned a specialist by slash at all (docs §226).
fn could_be_a_name(name: &str) -> bool {
    name.chars().all(is_name_char)
}

/// Split `/name rest` if that is what this is.
///
/// A leading slash and nothing else is still a command — that is the moment the picker should
/// open, before there is anything to match on.
pub fn parse(input: &str) -> Option<Command> {
    let rest = input.strip_prefix('/')?;
    // Split on the first run of whitespace, so `/eda   do the thing` is not read as a name of
    // `eda` followed by a prompt that begins with two spaces.
    let (name, prompt) = match rest.find(char::is_whitespace) {
        Some(at) => (&rest[..at], rest[at..].trim_start()),
        None => (rest, ""),
    };
    if !could_be_a_name(name) {
        return None;
    }
    Some(Command {
        name: name.to_string(),
        prompt: prompt.to_string(),
    })
}

/// The specialists worth showing for a partly-typed name, best first.
///
/// An empty query lists everything in the order the backend assembled them, which is the order
/// they appear in the coordinator's own list — no scoring, so the picker opening on `/` shows a
/// stable menu rather than an arbitrary one.
pub fn ranked<'a>(query: &str, agents: &'a [Subagent]) -> Vec<&'a Subagent> {
    if query.is_empty() {
        return agents.iter().collect();
    }
    let mut scored: Vec<(i32, usize, &Subagent)> = agents
        .iter()
        .enumerate()
        .filter_map(|(index, agent)| score(query, agent).map(|score| (score, index, agent)))
        .collect();
    // Declaration order breaks ties, so equal matches keep the backend's ordering.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, agent)| agent).collect()
}

/// How well a query fits one specialist: its name, or failing that what it says it does.
///
/// The name is worth more. A description match is how `eda` finds
/// `exploratory_data_analysis` when someone types the acronym the description spells out, but a
/// specialist whose *name* matches is nearly always the one meant.
fn score(query: &str, agent: &Subagent) -> Option<i32> {
    let name = crate::match_score(query, &agent.name);
    let described = crate::match_score(query, &agent.description).map(|score| score / 2);
    match (name, described) {
        (Some(name), Some(described)) => Some(name.max(described)),
        (found, None) | (None, found) => found,
    }
}

/// Whether a name is one the backend will recognise.
pub fn known(name: &str, agents: &[Subagent]) -> bool {
    agents.iter().any(|agent| agent.name == name)
}

/// The turn to send for a `/subagent` command.
///
/// Written as an instruction to the coordinator rather than as a tool call, because that is what
/// it is — see the module docs. Names the subagent in the same spelling the registry uses, so
/// there is no gap between what was validated and what was asked for.
///
/// Both modes are instructions, differing only in what they ask for — there is no second code
/// path, because there is no endpoint either of them could call instead.
pub fn turn(name: &str, prompt: &str, dispatch: Dispatch) -> String {
    let prompt = prompt.trim();
    match dispatch {
        Dispatch::Foreground => {
            format!("Delegate this to the `{name}` subagent and report what it finds: {prompt}")
        }
        Dispatch::Background => format!(
            "Start background work with start_async_task, subagent_type `{name}`, and this \
             description: {prompt}. Tell me it has started and carry on — do not wait for it."
        ),
    }
}

/// How the specialist should be reached.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Dispatch {
    /// Runs in this turn. Right for a literature lookup — you are waiting for the answer.
    #[default]
    Foreground,
    /// Handed to a background Mini-Me, reporting into the Jobs panel (§31, §42). Right for an
    /// EDA or a report, and the only way to have three of them running at once.
    ///
    /// Reached from the command palette rather than from a syntax: whether work blocks is a
    /// property of the work, not something a researcher should have to encode in punctuation,
    /// and `/name!` would be a thing to memorise for no gain.
    Background,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Vec<Subagent> {
        // The real ten, from the fixture the overlay wrote.
        crate::workspace::parse_registry(include_str!("../tests/fixtures/subagent-registry.json"))
    }

    #[test]
    fn a_bare_slash_is_already_a_command() {
        // The picker has to open before there is anything to match, or completion is useless:
        // you would have to know the name to be shown the name.
        assert_eq!(
            parse("/"),
            Some(Command {
                name: String::new(),
                prompt: String::new()
            })
        );
        assert_eq!(parse("no slash here"), None);
        // A slash that is not at the start is a path, a date or a fraction.
        assert_eq!(parse("see data/raw.csv"), None);
    }

    #[test]
    fn the_name_stops_at_the_first_space_and_the_rest_is_the_prompt() {
        let command = parse("/exploratory_data_analysis make an EDA of data.csv").unwrap();
        assert_eq!(command.name, "exploratory_data_analysis");
        assert_eq!(command.prompt, "make an EDA of data.csv");
        // Extra spacing between the two belongs to neither.
        let padded = parse("/report_writer    write it up").unwrap();
        assert_eq!(padded.name, "report_writer");
        assert_eq!(padded.prompt, "write it up");
    }

    #[test]
    fn the_names_the_request_imagined_find_the_ones_the_backend_has() {
        // The whole reason matching also reads descriptions. `/eda-subagent`,
        // `/research-paper` and `/report-write` are what was asked for; none of them is a
        // backend name.
        let agents = registry();
        let first = |query: &str| ranked(query, &agents).first().map(|a| a.name.clone());
        assert_eq!(first("eda"), Some("exploratory_data_analysis".into()));
        assert_eq!(first("research"), Some("academic_researcher".into()));
        assert_eq!(first("report"), Some("report_writer".into()));
        assert_eq!(first("dataverse"), Some("dataverse_explorer".into()));
        assert_eq!(first("clean"), Some("data_cleaning".into()));
    }

    #[test]
    fn an_empty_query_lists_everything_in_the_backends_own_order() {
        let agents = registry();
        let listed = ranked("", &agents);
        assert_eq!(listed.len(), agents.len());
        assert_eq!(listed[0].name, agents[0].name);
    }

    #[test]
    fn a_query_matching_nothing_ranks_nothing() {
        // Which is what lets the composer refuse at send instead of shrugging.
        assert!(ranked("zzzzqqqq", &registry()).is_empty());
    }

    #[test]
    fn only_a_name_the_registry_carries_is_accepted() {
        let agents = registry();
        assert!(known("report_writer", &agents));
        // A typo, and the name the request guessed at. Both have to fail loudly at send: the
        // alternative is a ten-minute wait for a turn that was never delegated (§55).
        assert!(!known("report_writter", &agents));
        assert!(!known("report-write", &agents));
        assert!(!known("", &agents));
    }

    #[test]
    fn the_turn_names_the_subagent_in_the_registrys_own_spelling() {
        // No gap between what was validated and what was asked for.
        let sent = turn(
            "exploratory_data_analysis",
            "  do it  ",
            Dispatch::Foreground,
        );
        assert!(sent.contains("`exploratory_data_analysis`"), "{sent}");
        assert!(sent.ends_with("do it"), "{sent}");
        // Every registry name survives being put in a turn — including the underscored ones,
        // which is the whole set.
        for agent in registry() {
            let asked = turn(&agent.name, "x", Dispatch::Foreground);
            assert!(asked.contains(&agent.name), "{}", agent.name);
        }
    }

    #[test]
    fn background_asks_for_background_work_and_says_not_to_wait() {
        // Its whole value is that the conversation stays live, so the instruction has to say so:
        // a coordinator that starts the task and then blocks on it has given up the point.
        let sent = turn("report_writer", "write it up", Dispatch::Background);
        assert!(sent.contains("start_async_task"), "{sent}");
        assert!(sent.contains("`report_writer`"), "{sent}");
        assert!(sent.contains("do not wait"), "{sent}");
        assert_ne!(
            turn("report_writer", "write it up", Dispatch::Foreground),
            sent,
            "the two modes must ask for different things"
        );
    }

    /// The turn the researcher actually sent, verbatim from the report.
    #[test]
    fn an_attached_file_is_not_a_specialist() {
        let turn = "/mnt/c/Users/LENOVO/Downloads/Graph-neural-networks.pdf\n\
                    Use the pdf_librarian subagent to search GNN expressivity";
        assert!(
            parse(turn).is_none(),
            "a path must reach the agent as a path, not be rejected as a specialist name"
        );
    }

    #[test]
    fn a_windows_path_is_not_one_either() {
        assert!(parse("/C:\\Users\\LENOVO\\paper.pdf ask about it").is_none());
        assert!(parse("/home/piero/data.csv").is_none());
    }

    #[test]
    fn the_names_that_are_real_still_parse() {
        for name in ["pdf_librarian", "data_voyager", "academic_researcher", "eda2", "a-b"] {
            let command = parse(&format!("/{name} do the thing")).expect("a command");
            assert_eq!(command.name, name);
            assert_eq!(command.prompt, "do the thing");
        }
    }

    /// A half-typed name keeps working — the guard must not fire on a prefix.
    #[test]
    fn a_partly_typed_name_still_parses() {
        assert_eq!(parse("/pdf_l").expect("a command").name, "pdf_l");
    }

    /// A wrong name still reaches "did you mean", which is a better answer than silence.
    #[test]
    fn a_misspelled_name_is_still_a_command() {
        assert_eq!(parse("/pdflibrarain find it").expect("a command").name, "pdflibrarain");
    }
}

