//! The record of an enquiry: what was consulted, in what order, and where the work doubled back.
//!
//! Requested (docs §73) as a modal showing which subagents a conversation used and how it moved
//! between them, with one sentence as the specification:
//!
//! > paper search → theories → get data → clean data → analyze data → theories → paper search
//! >
//! > "This shows how in reality science works, so each scientist can track his work by
//! > conversation."
//!
//! The doubling back is the part worth building for. A run that goes out to the literature, forms
//! a theory, gets data, analyses it and *returns to theories with what it found* is not a pipeline
//! that malfunctioned — it is the loop the method is made of, and it is invisible today.
//!
//! # Why this module holds data and not pixels
//!
//! Because the record is the hard half. `Message::agents` already carries every invocation of a
//! live turn, but `conversation_messages` returns role and text only — the activity trace does not
//! survive a reload, deliberately (§46: it was assembled from a stream that is over). That is the
//! right call for the transcript and fatal here: a graph that empties when you reopen the
//! conversation fails at exactly the thing it was asked for. So the client writes what it sees, as
//! it sees it, to the thread's own directory — it is the only thing that ever sees the stream.
//!
//! # Where the edges come from
//!
//! Two kinds, and they are not equally certain. Saying which is which is the whole reason a
//! researcher can use this (§73's third option: "a provenance record that quietly guesses is worse
//! than no provenance record, because it will be believed").
//!
//! - [`Edge::Delegated`] is **causal and true by construction.** LangGraph namespaces are
//!   `|`-joined paths (`NS_SEP = "|"`, `langgraph/_internal/_constants.py:87`), so a nested
//!   delegation arrives as `tools:a|tools:b` and the parent of any node is its namespace minus the
//!   last segment. §73 called getting this "a small upstream question worth asking"; §75 found
//!   there was nothing to ask — it has been arriving since the beginning, and the client already
//!   kept the whole namespace and used it only as a grouping key.
//! - [`Edge::Then`] is **observed order, not causation.** Nesting says two siblings share a
//!   parent; it says nothing about which ran first. For that there is only arrival, and §74's rule
//!   is the honest one: *overlap proves concurrency, a gap only suggests sequence.* An arrival
//!   interval is narrower than the execution it stands for — the first token lands after the agent
//!   started, the last before it stopped — so two invocations whose intervals overlap certainly
//!   ran together, while a gap leaves room for one to have been working silently.
//!
//! Across turns the ordering needs no hedge at all: the researcher read one answer and then typed
//! the next question. That is where the loop lives, which is why §75 concluded *cycles across
//! turns, a tree within one* — a truer picture than a single flat graph, and one that falls out of
//! the data rather than being imposed on it.
//!
//! # What is deliberately not here
//!
//! Duration comes from arrival stamps because it cannot come from anywhere else:
//! `langgraph_api/stream.py:262` yields the metadata chunk as exactly
//! `{"run_id": run_id, "attempt": attempt}`, byte-identical to the first frame of the captured
//! fixture, so on this point the capture was not reduced and the wire genuinely carries no time
//! (§75). `created_at` exists in `langgraph_api/schema.py`, but on runs and threads — REST
//! resources — never on a stream event.

use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Written into the thread's own directory, beside the outputs that turn produced (§42) — the
/// directory a researcher is already pointed at, so no new location has to be explained.
pub const FILENAME: &str = "provenance.json";

/// Versioned like the subagent registry, and for the same reason: a client reading a shape it does
/// not understand should say so rather than draw a plausible wrong picture.
pub const FORMAT: u64 = 1;

/// Wall-clock milliseconds since the epoch.
///
/// `SystemTime` rather than `Instant` because this is written to disk and read back in a later
/// process, where a monotonic clock reading means nothing.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or_default()
}

/// One conversation's provenance.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    #[serde(default)]
    pub turns: Vec<Turn>,
}

/// One question and the work it set off.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    /// The question as the researcher typed it. The row heading — without it a timeline is a
    /// list of specialists with no account of what was being asked.
    #[serde(default)]
    pub prompt: String,
    /// When it was sent.
    #[serde(default)]
    pub sent_at: u64,
    /// Every subagent invocation beneath it, in first-seen order.
    #[serde(default)]
    pub invocations: Vec<Invocation>,
}

/// One invocation of one specialist.
///
/// An *invocation*, not a kind: two concurrent runs of the same subagent are two entries here and
/// one node in the [`Graph`]. That distinction is what makes a revisit visible — `theories`
/// appearing twice is one node visited twice, which is the whole of §73's example.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invocation {
    /// The specialist's own name, as the backend sends it (`lc_agent_name`, §15b).
    #[serde(default)]
    pub name: String,
    /// The pregel checkpoint namespace — `|`-joined, unique per invocation, and the source of
    /// every causal edge in the graph.
    #[serde(default)]
    pub ns: String,
    /// First token seen from this invocation.
    #[serde(default)]
    pub first_seen: u64,
    /// Last token seen. Equal to `first_seen` for an invocation that produced one chunk.
    #[serde(default)]
    pub last_seen: u64,
}

impl Invocation {
    /// The namespace that delegated to this one, if it was itself a delegation.
    ///
    /// `None` for a top-level invocation, whose parent is the coordinator — which is not a
    /// subagent and so is not a node.
    pub fn parent(&self) -> Option<&str> {
        let (parent, _) = self.ns.rsplit_once('|')?;
        Some(parent)
    }
}

impl Record {
    /// Start recording a new question.
    pub fn begin_turn(&mut self, prompt: impl Into<String>, sent_at: u64) {
        self.turns.push(Turn {
            prompt: prompt.into(),
            sent_at,
            invocations: Vec::new(),
        });
    }

    /// Note that `ns` produced something at `at`.
    ///
    /// Idempotent per namespace: the first call opens the interval, every later one extends it.
    /// Called once per streamed chunk, so it must stay cheap — the linear scan is over one turn's
    /// invocations, which is single digits.
    ///
    /// Silently ignored when no turn has begun. That is not laziness: subagent frames can arrive
    /// while the client is still catching up on a resumed run, and inventing a turn with no
    /// question attached would put an unexplained row in the researcher's record.
    pub fn observe(&mut self, ns: &str, name: &str, at: u64) {
        let Some(turn) = self.turns.last_mut() else {
            return;
        };
        if let Some(existing) = turn
            .invocations
            .iter_mut()
            .find(|invocation| invocation.ns == ns)
        {
            existing.last_seen = at.max(existing.last_seen);
            // A namespace's first frame can arrive before the metadata that names it, in which
            // case the decoder falls back to "subagent". Take the real name when it turns up.
            if existing.name == FALLBACK_NAME && name != FALLBACK_NAME {
                existing.name = name.to_string();
            }
            return;
        }
        turn.invocations.push(Invocation {
            name: name.to_string(),
            ns: ns.to_string(),
            first_seen: at,
            last_seen: at,
        });
    }

    /// Note that a **background** worker produced something, now.
    ///
    /// Separate from [`Self::observe`] for one reason: background work outlives the turn that
    /// started it. A theorizer launched in turn three is still running during turns four and
    /// five, and `observe` — which looks only at the turn in progress — would file it three
    /// times, as three different pieces of work. So this searches every turn, and extends the
    /// entry wherever it already lives.
    ///
    /// It is why the provenance record showed nothing for async subagents at all: they run on
    /// their **own LangGraph thread**, so not one of their events reaches the conversation's
    /// stream (§43). The only trace on this side is the `async_tasks` map in each snapshot —
    /// which the client already decoded, for the Jobs panel, and never told the record about
    /// (docs §111).
    pub fn observe_background(&mut self, ns: &str, name: &str, at: u64) {
        for turn in self.turns.iter_mut() {
            if let Some(existing) = turn.invocations.iter_mut().find(|i| i.ns == ns) {
                existing.last_seen = at.max(existing.last_seen);
                if existing.name == FALLBACK_NAME && name != FALLBACK_NAME {
                    existing.name = name.to_string();
                }
                return;
            }
        }
        self.observe(ns, name, at);
    }

    /// Whether there is anything worth showing.
    ///
    /// A conversation of undelegated turns has a record, and it is empty of work — the modal
    /// should say so rather than draw an empty canvas.
    pub fn is_empty(&self) -> bool {
        self.turns.iter().all(|turn| turn.invocations.is_empty())
    }

    /// The span a timeline should draw full-width: the longest turn in the conversation.
    ///
    /// **Shared across every row, which is the whole correction.** The timeline first normalised
    /// each turn against its own span, so a turn holding one invocation always drew a full-width
    /// bar — an 8-second lookup and a 32-second one came out pixel-identical, side by side, and
    /// the view invited exactly the comparison it could not support. A chart whose bars carry no
    /// information is worse than no chart, because it will be read anyway.
    ///
    /// Turn *spans* rather than individual durations, because a turn's bars are laid out inside
    /// it: a scale smaller than the span would push a later sibling past the end of its row.
    /// Never zero, so the caller can divide by it.
    pub fn scale(&self) -> u64 {
        self.turns
            .iter()
            .filter_map(|turn| {
                let start = turn.invocations.iter().map(|i| i.first_seen).min()?;
                let end = turn.invocations.iter().map(|i| i.last_seen).max()?;
                Some(end.saturating_sub(start))
            })
            .max()
            .unwrap_or(0)
            .max(1)
    }

    /// Collapse invocations into kinds, and derive the edges between them.
    pub fn graph(&self) -> Graph {
        let mut nodes: Vec<Node> = Vec::new();
        let mut index: HashMap<&str, usize> = HashMap::new();
        for invocation in self.turns.iter().flat_map(|turn| &turn.invocations) {
            let at = *index.entry(&invocation.name).or_insert_with(|| {
                nodes.push(Node {
                    name: invocation.name.clone(),
                    visits: 0,
                });
                nodes.len() - 1
            });
            nodes[at].visits += 1;
        }

        let mut tally: HashMap<(usize, usize, Edge), usize> = HashMap::new();
        let mut note = |from: usize, to: usize, kind: Edge| {
            // A specialist delegating to itself, or following itself across turns, is a real
            // observation but not a drawable edge — it is a visit count, which the node carries.
            if from != to {
                *tally.entry((from, to, kind)).or_insert(0) += 1;
            }
        };

        let mut previous_last: Vec<usize> = Vec::new();
        for turn in &self.turns {
            let of = |invocation: &Invocation| index[invocation.name.as_str()];

            // Causal edges: who delegated to whom, straight off the namespace path.
            let by_ns: HashMap<&str, &Invocation> = turn
                .invocations
                .iter()
                .map(|invocation| (invocation.ns.as_str(), invocation))
                .collect();
            for invocation in &turn.invocations {
                if let Some(parent) = invocation.parent().and_then(|ns| by_ns.get(ns)) {
                    note(of(parent), of(invocation), Edge::Delegated);
                }
            }

            // Observed order: within each set of siblings, and across the turn boundary.
            let mut first: Vec<usize> = Vec::new();
            let mut last: Vec<usize> = Vec::new();
            for group in siblings(turn) {
                let bands = bands(&group);
                if let (Some(head), Some(tail)) = (bands.first(), bands.last()) {
                    first.extend(head.iter().map(|invocation| of(invocation)));
                    last.extend(tail.iter().map(|invocation| of(invocation)));
                }
                for pair in bands.windows(2) {
                    for before in &pair[0] {
                        for after in &pair[1] {
                            note(of(before), of(after), Edge::Then);
                        }
                    }
                }
            }
            // The turn boundary needs no hedge: the researcher read one answer before typing the
            // next question, so this ordering is a fact about a person, not an inference about a
            // scheduler. It is also where §73's loop lives.
            for before in &previous_last {
                for after in &first {
                    note(*before, *after, Edge::Then);
                }
            }
            if !last.is_empty() {
                previous_last = last;
            }
        }

        let mut edges: Vec<Traversal> = tally
            .into_iter()
            .map(|((from, to, kind), count)| Traversal {
                from,
                to,
                kind,
                count,
            })
            .collect();
        // Stable output: a HashMap's order is not, and a graph that redraws differently on every
        // open is one a reader cannot learn.
        edges.sort_by_key(|edge| (edge.from, edge.to, edge.kind));
        Graph { nodes, edges }
    }
}

/// What the decoder calls a subagent it could not name.
///
/// Mirrors `protocol::agent_ref`'s fallback. Kept as a constant here so the two cannot drift into
/// disagreeing about which invocations are still waiting for a name.
const FALLBACK_NAME: &str = "subagent";

/// Group a turn's invocations by who delegated them, parents before children.
///
/// The top-level group — everything the coordinator dispatched — comes first, which is the order
/// the work reads in.
fn siblings(turn: &Turn) -> Vec<Vec<&Invocation>> {
    let mut order: Vec<&str> = Vec::new();
    let mut groups: HashMap<&str, Vec<&Invocation>> = HashMap::new();
    for invocation in &turn.invocations {
        let parent = invocation.parent().unwrap_or("");
        if !groups.contains_key(parent) {
            order.push(parent);
        }
        groups.entry(parent).or_default().push(invocation);
    }
    order
        .into_iter()
        .filter_map(|parent| groups.remove(parent))
        .collect()
}

/// Partition siblings into runs that provably did not overlap.
///
/// Everything in one band was running at the same time as something else in that band; everything
/// in band *n* had finished before anything in band *n + 1* began. That is the strongest true
/// statement available about sibling order (§74), and it is what keeps two subagents dispatched
/// together from being drawn as a chain.
///
/// Touching intervals separate: one that begins at the exact millisecond another ends is the
/// sequential case, read at the only resolution we have.
fn bands<'a>(group: &[&'a Invocation]) -> Vec<Vec<&'a Invocation>> {
    let mut sorted: Vec<&Invocation> = group.to_vec();
    sorted.sort_by_key(|invocation| (invocation.first_seen, invocation.last_seen));
    let mut bands: Vec<Vec<&Invocation>> = Vec::new();
    let mut reach = 0u64;
    for invocation in sorted {
        match bands.last_mut() {
            // Starts before the band's furthest end: it was running while something in the band
            // still was, so it belongs to the band.
            Some(band) if invocation.first_seen < reach => {
                band.push(invocation);
                reach = reach.max(invocation.last_seen);
            }
            _ => {
                reach = invocation.last_seen;
                bands.push(vec![invocation]);
            }
        }
    }
    bands
}

/// Kinds and the transitions between them — a projection of the record, never a second source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Graph {
    /// One per specialist kind, in first-appearance order.
    pub nodes: Vec<Node>,
    pub edges: Vec<Traversal>,
}

/// A kind of specialist, and how often it was visited.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub visits: usize,
}

/// An observed transition and how many times it happened.
///
/// `theories → paper search` traversed three times is one edge that says three, not three edges
/// (§73).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Traversal {
    pub from: usize,
    pub to: usize,
    pub kind: Edge,
    pub count: usize,
}

/// How much an edge can be trusted. Drawn differently, and explained in the modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Edge {
    /// One delegated to the other. True by construction from the namespace path.
    Delegated,
    /// One was seen to finish before the other started. Order, not cause.
    Then,
}

impl Edge {
    /// How the modal says it, in the researcher's terms rather than the engine's.
    pub fn label(self) -> &'static str {
        match self {
            Edge::Delegated => "delegated to",
            Edge::Then => "then",
        }
    }
}

/// Read a conversation's record. Absent, unreadable or from a shape we do not know reads as empty.
///
/// Never an error: this is a record of past work shown beside live work, and a conversation whose
/// provenance file was hand-edited into nonsense should still open.
pub fn load(dir: &Path) -> Record {
    let Ok(text) = std::fs::read_to_string(dir.join(FILENAME)) else {
        return Record::default();
    };
    parse(&text)
}

/// Separated from the read so the shape can be tested without a filesystem.
pub(crate) fn parse(text: &str) -> Record {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Record::default();
    };
    if value.get("format").and_then(serde_json::Value::as_u64) != Some(FORMAT) {
        return Record::default();
    }
    serde_json::from_value(value).unwrap_or_default()
}

/// Write a conversation's record, whole.
///
/// Written to a temporary and renamed, so a reader never sees half a record — the same discipline
/// the overlay uses for the subagent registry, and for the same reason: this file is read by
/// another process at a moment of its choosing.
pub fn save(dir: &Path, record: &Record) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating {} for the provenance record", dir.display()))?;
    let path = dir.join(FILENAME);
    let mut value = serde_json::to_value(record)?;
    value
        .as_object_mut()
        .expect("a Record serialises to an object")
        .insert("format".into(), FORMAT.into());
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_string_pretty(&value)?)
        .with_context(|| format!("writing {}", temporary.display()))?;
    std::fs::rename(&temporary, &path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A turn that delegated to `first` then `second`, one after the other.
    fn sequential() -> Record {
        let mut record = Record::default();
        record.begin_turn("search deseq2 paper", 1_000);
        record.observe("tools:a", "academic_researcher", 1_100);
        record.observe("tools:a", "academic_researcher", 1_500);
        record.observe("tools:b", "theorizer", 2_000);
        record.observe("tools:b", "theorizer", 2_400);
        record
    }

    #[test]
    fn a_namespace_path_names_its_own_parent() {
        // The whole basis of the causal edge, and the thing §73 assumed had to be asked upstream.
        let nested = Invocation {
            ns: "tools:a|tools:b".into(),
            ..Invocation::default()
        };
        assert_eq!(nested.parent(), Some("tools:a"));
        let top = Invocation {
            ns: "tools:a".into(),
            ..Invocation::default()
        };
        // The coordinator is not a subagent, so a top-level invocation has no parent *node*.
        assert_eq!(top.parent(), None);
    }

    #[test]
    fn observing_the_same_namespace_extends_its_interval_rather_than_repeating_it() {
        let record = sequential();
        let [first, second] = record.turns[0].invocations.as_slice() else {
            panic!("expected two invocations, got {:?}", record.turns[0]);
        };
        assert_eq!((first.first_seen, first.last_seen), (1_100, 1_500));
        assert_eq!((second.first_seen, second.last_seen), (2_000, 2_400));
    }

    #[test]
    fn a_name_arriving_late_replaces_the_placeholder() {
        // The first frame of a namespace can land before the metadata that names it, and a node
        // labelled "subagent" for the rest of the conversation would be a permanent scar from a
        // millisecond of ignorance.
        let mut record = Record::default();
        record.begin_turn("q", 0);
        record.observe("tools:a", "subagent", 10);
        record.observe("tools:a", "data_cleaning", 20);
        assert_eq!(record.turns[0].invocations[0].name, "data_cleaning");
    }

    #[test]
    fn frames_arriving_before_any_question_are_dropped() {
        // Rather than inventing a turn with no prompt: an unexplained row in a record of one's
        // own work is worse than a missing one.
        let mut record = Record::default();
        record.observe("tools:a", "academic_researcher", 10);
        assert!(record.turns.is_empty());
        assert!(record.is_empty());
    }

    #[test]
    fn nesting_is_a_delegation_edge() {
        let mut record = Record::default();
        record.begin_turn("write it up", 0);
        record.observe("tools:a", "report_writer", 10);
        record.observe("tools:a|tools:b", "academic_researcher", 20);
        let graph = record.graph();
        assert_eq!(graph.nodes.len(), 2);
        let [edge] = graph.edges.as_slice() else {
            panic!("expected one edge, got {:?}", graph.edges);
        };
        assert_eq!(edge.kind, Edge::Delegated);
        assert_eq!(graph.nodes[edge.from].name, "report_writer");
        assert_eq!(graph.nodes[edge.to].name, "academic_researcher");
    }

    #[test]
    fn siblings_that_overlap_get_no_edge_between_them() {
        // The case §73 worried about and §74 fixed: two subagents dispatched together must not be
        // drawn as a chain just because one's tokens arrived first.
        let mut record = Record::default();
        record.begin_turn("do both", 0);
        record.observe("tools:a", "academic_researcher", 100);
        record.observe("tools:b", "dataverse_explorer", 150);
        record.observe("tools:a", "academic_researcher", 400);
        record.observe("tools:b", "dataverse_explorer", 500);
        assert!(
            record.graph().edges.is_empty(),
            "overlapping siblings ran together; there is no 'then' between them"
        );
    }

    #[test]
    fn siblings_that_do_not_overlap_get_an_observed_order() {
        let graph = sequential().graph();
        let [edge] = graph.edges.as_slice() else {
            panic!("expected one edge, got {:?}", graph.edges);
        };
        assert_eq!(edge.kind, Edge::Then);
        assert_eq!(graph.nodes[edge.from].name, "academic_researcher");
        assert_eq!(graph.nodes[edge.to].name, "theorizer");
    }

    #[test]
    fn a_band_of_concurrent_work_connects_as_a_whole_to_what_follows_it() {
        // Two ran together, then a third started after both had finished. The true statement is
        // "both of those, then this" — two edges, and none between the pair.
        let mut record = Record::default();
        record.begin_turn("q", 0);
        record.observe("tools:a", "academic_researcher", 100);
        record.observe("tools:b", "dataverse_explorer", 150);
        record.observe("tools:a", "academic_researcher", 400);
        record.observe("tools:b", "dataverse_explorer", 500);
        record.observe("tools:c", "data_cleaning", 600);
        record.observe("tools:c", "data_cleaning", 700);
        let graph = record.graph();
        assert_eq!(graph.edges.len(), 2, "{:?}", graph.edges);
        assert!(graph.edges.iter().all(|edge| edge.kind == Edge::Then));
        assert!(graph
            .edges
            .iter()
            .all(|edge| graph.nodes[edge.to].name == "data_cleaning"));
    }

    #[test]
    fn returning_to_a_kind_in_a_later_turn_is_the_cycle() {
        // §73's example, reduced: the return to `theories` is what the whole feature is for, and
        // it must come out as a visited-twice node with an edge back into it — not as two nodes.
        let mut record = Record::default();
        record.begin_turn("find papers", 0);
        record.observe("tools:a", "academic_researcher", 100);
        record.observe("tools:a", "academic_researcher", 200);
        record.begin_turn("theorise from them", 300);
        record.observe("tools:b", "theorizer", 400);
        record.observe("tools:b", "theorizer", 500);
        record.begin_turn("now find papers on that", 600);
        record.observe("tools:c", "academic_researcher", 700);
        record.observe("tools:c", "academic_researcher", 800);
        let graph = record.graph();
        assert_eq!(graph.nodes.len(), 2, "a revisit is one node, not two");
        let researcher = graph
            .nodes
            .iter()
            .find(|node| node.name == "academic_researcher")
            .expect("the researcher is a node");
        assert_eq!(researcher.visits, 2, "visited twice");
        // Both directions traversed once each — which is the loop, drawn.
        assert_eq!(graph.edges.len(), 2, "{:?}", graph.edges);
        assert!(graph.edges.iter().all(|edge| edge.count == 1));
        assert!(graph
            .edges
            .iter()
            .any(|edge| graph.nodes[edge.from].name == "academic_researcher"
                && graph.nodes[edge.to].name == "theorizer"));
        assert!(graph
            .edges
            .iter()
            .any(|edge| graph.nodes[edge.from].name == "theorizer"
                && graph.nodes[edge.to].name == "academic_researcher"));
    }

    #[test]
    fn the_same_transition_twice_is_one_edge_that_counts_to_two() {
        let mut record = Record::default();
        for turn in 0..2 {
            let base = turn * 1_000;
            record.begin_turn("q", base);
            record.observe("tools:a", "academic_researcher", base + 100);
            record.observe("tools:a", "academic_researcher", base + 200);
            record.observe("tools:b", "theorizer", base + 300);
            record.observe("tools:b", "theorizer", base + 400);
        }
        let graph = record.graph();
        let researcher_to_theorizer = graph
            .edges
            .iter()
            .filter(|edge| {
                graph.nodes[edge.from].name == "academic_researcher"
                    && graph.nodes[edge.to].name == "theorizer"
            })
            .collect::<Vec<_>>();
        assert_eq!(researcher_to_theorizer.len(), 1, "one edge, not two");
        assert_eq!(researcher_to_theorizer[0].count, 2, "traversed twice");
    }

    #[test]
    fn a_turn_that_delegated_to_nothing_does_not_break_the_chain_across_it() {
        // A researcher asking a plain question mid-enquiry should not sever the record of what
        // came before from what comes after.
        let mut record = Record::default();
        record.begin_turn("find papers", 0);
        record.observe("tools:a", "academic_researcher", 100);
        record.observe("tools:a", "academic_researcher", 200);
        record.begin_turn("what does that mean?", 300);
        record.begin_turn("theorise from them", 600);
        record.observe("tools:b", "theorizer", 700);
        record.observe("tools:b", "theorizer", 800);
        let graph = record.graph();
        assert_eq!(graph.edges.len(), 1, "{:?}", graph.edges);
        assert_eq!(graph.nodes[graph.edges[0].from].name, "academic_researcher");
    }

    #[test]
    fn a_kind_following_itself_is_a_visit_count_not_a_self_loop() {
        let mut record = Record::default();
        record.begin_turn("find papers", 0);
        record.observe("tools:a", "academic_researcher", 100);
        record.observe("tools:a", "academic_researcher", 200);
        record.begin_turn("find more", 300);
        record.observe("tools:b", "academic_researcher", 400);
        record.observe("tools:b", "academic_researcher", 500);
        let graph = record.graph();
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].visits, 2);
        assert!(
            graph.edges.is_empty(),
            "an arrow from a node to itself says nothing the visit count does not"
        );
    }

    #[test]
    fn a_record_survives_the_round_trip_it_is_written_for() {
        let record = sequential();
        let directory =
            std::env::temp_dir().join(format!("mini-me-provenance-{}", std::process::id()));
        save(&directory, &record).expect("writing the record");
        assert_eq!(load(&directory), record);
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_shape_we_do_not_know_reads_as_empty_rather_than_as_something() {
        // Same discipline as the subagent registry: a record half-understood would be drawn as
        // though it were complete, and this one is meant to be trusted.
        assert_eq!(parse("not json at all"), Record::default());
        assert_eq!(parse(r#"{"turns": []}"#), Record::default(), "no format");
        assert_eq!(
            parse(r#"{"format": 99, "turns": [{"prompt": "q"}]}"#),
            Record::default(),
            "a newer format"
        );
        // The current one, with a field it has never heard of, still reads.
        let forward = parse(r#"{"format": 1, "turns": [{"prompt": "q", "invented": 1}]}"#);
        assert_eq!(forward.turns.len(), 1);
        assert_eq!(forward.turns[0].prompt, "q");
    }

    #[test]
    fn one_scale_serves_every_row_so_a_long_turn_looks_long() {
        // The bug this replaced: each turn normalised against its own span, so a turn holding a
        // single invocation always drew a full-width bar. Reported as "this doesn't make sense
        // because I asked these in two different prompts" — an 8s lookup and a 32s one, drawn
        // identically, one above the other (docs §85).
        let mut record = Record::default();
        record.begin_turn("search the deseq2 paper", 0);
        record.observe("tools:a", "academic_researcher", 1_000);
        record.observe("tools:a", "academic_researcher", 9_200); // 8.2s
        record.begin_turn("search 1 dataset", 20_000);
        record.observe("tools:b", "dataverse_explorer", 21_000);
        record.observe("tools:b", "dataverse_explorer", 53_400); // 32.4s

        let scale = record.scale();
        assert_eq!(scale, 32_400, "the longest turn sets the full width");
        let width = |turn: &Turn| {
            let invocation = &turn.invocations[0];
            invocation.last_seen.saturating_sub(invocation.first_seen) as f64 / scale as f64
        };
        // The shorter one must be visibly shorter. Under the old per-turn scale both were 1.0.
        assert!(
            (width(&record.turns[0]) - 0.253).abs() < 0.01,
            "{}",
            width(&record.turns[0])
        );
        assert!((width(&record.turns[1]) - 1.0).abs() < 0.001);
        // The idle minutes between one answer and the next question stay out of the scale, or
        // every bar would be a sliver.
        assert!(
            scale < 53_400,
            "the gap between turns is not part of any span"
        );
    }

    #[test]
    fn a_turn_lays_its_own_siblings_out_within_the_shared_scale() {
        // The reason the divisor is a turn *span* and not the longest single invocation: a later
        // sibling is offset by where it started, and a smaller scale would push it off the row.
        let mut record = Record::default();
        record.begin_turn("do both", 0);
        record.observe("tools:a", "academic_researcher", 0);
        record.observe("tools:a", "academic_researcher", 30_000);
        record.observe("tools:b", "theorizer", 30_000);
        record.observe("tools:b", "theorizer", 60_000);
        let scale = record.scale() as f64;
        let second = &record.turns[0].invocations[1];
        let offset = second.first_seen as f64 / scale;
        let width = (second.last_seen - second.first_seen) as f64 / scale;
        assert!(
            offset + width <= 1.0001,
            "{offset} + {width} must fit the row"
        );
    }

    #[test]
    fn background_work_is_one_invocation_however_many_turns_it_outlives() {
        // A background worker runs on its own thread, so the only trace on this side is the
        // `async_tasks` map, repeated in every snapshot for as long as the task lives — across
        // the turn that started it and every turn after. Filing it per turn would show one
        // theorizer run as three separate pieces of work (docs §111).
        let mut record = Record::default();
        record.begin_turn("analyse this", 0);
        record.observe_background("async:t-1", "data_voyager", 100);
        record.observe_background("async:t-1", "data_voyager", 400);
        record.begin_turn("anything running?", 1_000);
        record.observe_background("async:t-1", "data_voyager", 1_100);
        record.begin_turn("still?", 2_000);
        record.observe_background("async:t-1", "data_voyager", 2_500);

        // One entry, in the turn that started it, spanning the whole time it was alive.
        assert_eq!(record.turns[0].invocations.len(), 1);
        assert!(record.turns[1].invocations.is_empty());
        assert!(record.turns[2].invocations.is_empty());
        let task = &record.turns[0].invocations[0];
        assert_eq!(task.name, "data_voyager");
        assert_eq!((task.first_seen, task.last_seen), (100, 2_500));

        // And it is one node, visited once — not three.
        let graph = record.graph();
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].visits, 1);
    }

    #[test]
    fn a_second_background_task_is_its_own_invocation() {
        let mut record = Record::default();
        record.begin_turn("do two things", 0);
        record.observe_background("async:t-1", "data_voyager", 100);
        record.observe_background("async:t-2", "report_writer", 150);
        record.observe_background("async:t-1", "data_voyager", 900);
        record.observe_background("async:t-2", "report_writer", 950);
        assert_eq!(record.turns[0].invocations.len(), 2);
        // Overlapping, so no order is claimed between them — they were handed off together.
        assert!(
            record.graph().edges.is_empty(),
            "{:?}",
            record.graph().edges
        );
    }

    #[test]
    fn a_record_of_questions_that_delegated_nothing_is_empty() {
        let mut record = Record::default();
        record.begin_turn("hola", 0);
        record.begin_turn("how are you", 100);
        assert!(record.is_empty(), "two turns, no work to show");
        assert!(record.graph().nodes.is_empty());
    }
}
