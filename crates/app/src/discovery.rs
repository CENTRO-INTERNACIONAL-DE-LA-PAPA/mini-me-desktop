//! AutoDiscovery experiments, and the tree they form.
//!
//! An AutoDiscovery run is not a question with an answer. It is an MCTS search over hypotheses: each
//! experiment writes and runs its own code, measures how much the result moved the model's belief,
//! and spawns children from whatever looked worth refining. So the result is a *tree* of measured
//! belief shifts, and the panel's job is to make that tree readable.
//!
//! **Everything here is decoded from a captured payload, not from the docs.** §247 ran a real
//! 5-experiment probe and found nine places where the plan written from the documentation was wrong.
//! Six of those are defended against in this module, and every one has a test built on the frozen
//! response in `tests/fixtures/autodiscovery-experiments.json`:
//!
//! 1. **`surprise` is signed.** Its sign *is* the direction of the belief shift. Nothing derives
//!    direction by subtracting `prior` from `posterior` — the magnitudes do not agree (experiment
//!    `node_2_0` moves 0.360 and reports 0.671).
//! 2. **`is_surprising` is not a threshold on `|surprise|`.** With `surprisal_width: 0.5`, an
//!    experiment at −0.6705 came back `false`. The flag is read and never recomputed.
//! 3. **The belief labels are categories, not buckets of a number.** `prior_belief` is a
//!    distribution with *vote counts* over four labels, so the label is the counts' argmax — not a
//!    quartile of the mean.
//! 4. **`parent_id` can name a node that is not in the set.** Every experiment in the probe descends
//!    from `node_1_0`, which is not one of them. A dangling parent is a root, not corruption.
//! 5. **Edges come from `parent_id` only.** `child_ids` is populated by the list endpoint and
//!    *empty* by the detail endpoint — the same field with two answers and no way to tell from the
//!    response which you have. One direction of truth cannot disagree with itself.
//! 6. **Three identifiers, not interchangeable.** `experiment_id` (`node_2_0`) is what edges
//!    reference, `id_in_run` is what a human is shown, `creation_idx` is the order things happened.

// Decoded, laid out and tested against the frozen probe response, and not yet read by the panel —
// the tree it draws is the next commit. The allow comes off with the drawing.
#![allow(dead_code)]

use serde_json::Value;

/// How confident the model is that a hypothesis holds.
///
/// The four categories the service's belief distributions are defined over. Ordered false-to-true so
/// a comparison means what it looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    LikelyFalse,
    MaybeFalse,
    MaybeTrue,
    LikelyTrue,
}

impl Confidence {
    /// The wording AutoDiscovery's own web view uses, so a researcher reading both sees one
    /// vocabulary. Its `Likely` is the payload's `definitely`.
    pub fn label(self) -> &'static str {
        match self {
            Confidence::LikelyFalse => "Likely False",
            Confidence::MaybeFalse => "Maybe False",
            Confidence::MaybeTrue => "Maybe True",
            Confidence::LikelyTrue => "Likely True",
        }
    }

    /// The payload key each category's vote count arrives under.
    fn key(self) -> &'static str {
        match self {
            Confidence::LikelyFalse => "definitely_false",
            Confidence::MaybeFalse => "maybe_false",
            Confidence::MaybeTrue => "maybe_true",
            Confidence::LikelyTrue => "definitely_true",
        }
    }

    const ALL: [Confidence; 4] = [
        Confidence::LikelyFalse,
        Confidence::MaybeFalse,
        Confidence::MaybeTrue,
        Confidence::LikelyTrue,
    ];
}

/// A belief about one hypothesis: the label its distribution favours, and its mean.
///
/// Two numbers arrive — `mean` and `_empirical_mean` — and they disagree (0.7917 against 0.85 in the
/// probe). This keeps `mean`, the one whose magnitude matches what the service's own view prints,
/// and says so here rather than leaving the choice looking arbitrary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Belief {
    pub label: Confidence,
    pub mean: f64,
}

impl Belief {
    /// Decode a `prior_belief` / `posterior_belief` object.
    ///
    /// The label is the argmax of the four vote counts. Ties break toward the *less* committed
    /// category, because claiming `Likely True` off a tie with `Maybe True` overstates what the
    /// distribution says — and this number ends up next to a hypothesis a researcher may act on.
    fn decode(value: Option<&Value>) -> Option<Self> {
        let object = value?.as_object()?;
        let mean = object.get("mean").and_then(Value::as_f64)?;
        let mut best: Option<(Confidence, f64)> = None;
        for candidate in Confidence::ALL {
            let votes = object
                .get(candidate.key())
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            // `>` not `>=`, and ALL runs false-to-true, so an equal count leaves the earlier
            // (less committed) label standing.
            if best.is_none_or(|(_, most)| votes > most) {
                best = Some((candidate, votes));
            }
        }
        let (label, votes) = best?;
        // All four counts zero says nothing about which label holds. Fall back to the mean rather
        // than reporting `Likely False` for an empty distribution.
        let label = if votes > 0.0 { label } else { from_mean(mean) };
        Some(Belief { label, mean })
    }
}

/// The label a bare mean implies, for a distribution that carries no votes at all.
///
/// Quarters — which is what §246 wrongly assumed the labels *always* were. It is a fallback for an
/// empty distribution and nothing else; whenever counts exist they decide.
fn from_mean(mean: f64) -> Confidence {
    match mean {
        m if m < 0.25 => Confidence::LikelyFalse,
        m if m < 0.5 => Confidence::MaybeFalse,
        m if m < 0.75 => Confidence::MaybeTrue,
        _ => Confidence::LikelyTrue,
    }
}

/// Which way an experiment moved the belief it tested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Toward,
    Away,
    Unmoved,
}

impl Direction {
    /// The word AutoDiscovery's own Direction column uses.
    pub fn label(self) -> &'static str {
        match self {
            Direction::Toward => "Positive",
            Direction::Away => "Negative",
            Direction::Unmoved => "—",
        }
    }
}

/// One experiment: a hypothesis, the code that tested it, and how far it moved the belief.
#[derive(Clone, Debug, PartialEq)]
pub struct Experiment {
    /// `experiment_id` — `node_2_0`. What every edge references.
    pub id: String,
    /// `id_in_run` — the small number a human is shown.
    pub number: u32,
    /// `creation_idx` — when it happened, and the only honest sort key.
    pub order: u32,
    /// `parent_id`, which may name a node outside this set (see the module note).
    pub parent: Option<String>,
    pub status: String,
    pub hypothesis: String,
    pub analysis: String,
    pub review: String,
    /// Signed. The sign is the direction; the magnitude is what a table column shows.
    pub surprise: Option<f64>,
    /// The server's own flag. Never recomputed from `surprise`.
    pub surprising: bool,
    pub prior: Option<Belief>,
    pub posterior: Option<Belief>,
    /// Seconds of compute, when reported.
    pub runtime: Option<f64>,
    /// Figures attached — **always 0 from the list endpoint**, which returns `rich_outputs: null`
    /// even for experiments that have them. Only a per-experiment fetch fills this in, at ~458KB
    /// each, which is why it is a count here and not the bytes.
    pub figures: usize,
}

impl Experiment {
    /// Which way the belief moved, read off the sign of `surprise`.
    pub fn direction(&self) -> Direction {
        match self.surprise {
            Some(value) if value > 0.0 => Direction::Toward,
            Some(value) if value < 0.0 => Direction::Away,
            _ => Direction::Unmoved,
        }
    }

    /// How big the shift was, for the column that ranks experiments.
    pub fn magnitude(&self) -> f64 {
        self.surprise.unwrap_or(0.0).abs()
    }

    pub fn is_finished(&self) -> bool {
        !matches!(
            self.status.to_ascii_uppercase().as_str(),
            "PENDING" | "RUNNING" | "IN_PROGRESS" | "CREATED" | ""
        )
    }

    pub fn succeeded(&self) -> bool {
        matches!(
            self.status.to_ascii_uppercase().as_str(),
            "SUCCEEDED" | "COMPLETED"
        )
    }
}

/// Decode an `experiments` response into experiments, ordered as they were created.
///
/// Shape-tolerant in one direction only: an entry with no `experiment_id` is dropped, because it
/// cannot be an edge endpoint and cannot be opened. Everything else degrades to a default rather
/// than losing the row — a run half-finished is the normal case while it is still working.
pub fn decode_experiments(payload: &Value) -> Vec<Experiment> {
    let Some(items) = payload.get("experiments").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut experiments: Vec<Experiment> = items
        .iter()
        .filter_map(|item| {
            let id = item
                .get("experiment_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())?;
            let text = |key: &str| {
                item.get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            };
            Some(Experiment {
                id: id.to_string(),
                number: item
                    .get("id_in_run")
                    .and_then(Value::as_u64)
                    .unwrap_or_default() as u32,
                order: item
                    .get("creation_idx")
                    .and_then(Value::as_u64)
                    .unwrap_or_default() as u32,
                parent: item
                    .get("parent_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|parent| !parent.is_empty())
                    .map(str::to_string),
                status: text("status"),
                hypothesis: text("hypothesis"),
                analysis: text("analysis"),
                review: text("review"),
                surprise: item.get("surprise").and_then(Value::as_f64),
                // Read, never derived. §247: −0.6705 with a 0.5 width came back false.
                surprising: item
                    .get("is_surprising")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                prior: Belief::decode(item.get("prior_belief")),
                posterior: Belief::decode(item.get("posterior_belief")),
                runtime: item
                    .get("runtime_ms")
                    .and_then(Value::as_f64)
                    .map(|ms| ms / 1000.0),
                figures: item
                    .get("rich_outputs")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0),
            })
        })
        .collect();
    experiments.sort_by_key(|experiment| (experiment.order, experiment.number));
    experiments
}

/// Whether the run has stopped producing experiments, as the response itself reports it.
pub fn finished(payload: &Value) -> bool {
    payload
        .get("has_job_completed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// A node placed for drawing: which experiment, how deep, and where across.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    /// Index into the slice that was laid out.
    pub at: usize,
    /// Distance from its root. Row.
    pub depth: usize,
    /// Position across, in units of one leaf. Fractional for a parent centred over its children.
    pub across: f64,
}

/// Lay the experiments out as a tidy tree: leaves take the next column, parents centre over theirs.
///
/// **Deterministic, and that is the point.** AutoDiscovery's own view is a force-directed graph of
/// this same data; a spring layout in this app would settle differently on every frame, and this
/// panel is rebuilt on every stream event. It also hides the one thing the tree says — depth is how
/// far the search kept refining a line of enquiry, and a blob has no depth.
///
/// Guards, each earned in §247:
/// - **Edges come from `parent` alone.** `child_ids` is complete in one response and empty in
///   another, so it is not read at all.
/// - **A parent outside the set is a root.** The probe's five experiments all descend from
///   `node_1_0`, which is not among them.
/// - **A cycle terminates.** Nothing in the payload promises acyclicity, and a parent chain that
///   loops would otherwise hang the window rather than draw a wrong picture.
pub fn layout(experiments: &[Experiment]) -> Vec<Placed> {
    use std::collections::HashMap;

    let index: HashMap<&str, usize> = experiments
        .iter()
        .enumerate()
        .map(|(at, experiment)| (experiment.id.as_str(), at))
        .collect();

    // Children, in creation order, keyed by the parent's own index. Built from `parent` only.
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); experiments.len()];
    let mut roots: Vec<usize> = Vec::new();
    for (at, experiment) in experiments.iter().enumerate() {
        match experiment
            .parent
            .as_deref()
            .and_then(|parent| index.get(parent))
        {
            // A self-parent is a one-node cycle; treat it as a root rather than its own child.
            Some(&parent) if parent != at => children[parent].push(at),
            _ => roots.push(at),
        }
    }

    let mut placed: Vec<Option<Placed>> = vec![None; experiments.len()];
    let mut next_column = 0.0_f64;
    // Iterative post-order, so a pathological depth cannot blow the stack the way recursion would.
    for &root in &roots {
        let mut stack = vec![(root, 0usize, false)];
        while let Some((at, depth, expanded)) = stack.pop() {
            if !expanded {
                if placed[at].is_some() {
                    continue; // already reached by another path — the cycle guard
                }
                // Claim it before descending, so a cycle back to `at` finds it taken.
                placed[at] = Some(Placed {
                    at,
                    depth,
                    across: 0.0,
                });
                stack.push((at, depth, true));
                for &child in children[at].iter().rev() {
                    stack.push((child, depth + 1, false));
                }
                continue;
            }
            let spans: Vec<f64> = children[at]
                .iter()
                .filter_map(|&child| placed[child].map(|node| node.across))
                .collect();
            let across = if spans.is_empty() {
                let column = next_column;
                next_column += 1.0;
                column
            } else {
                spans.iter().sum::<f64>() / spans.len() as f64
            };
            placed[at] = Some(Placed { at, depth, across });
        }
    }

    // Anything the walk never reached — only possible in a cycle with no entry point — still gets a
    // column, because a node the researcher cannot see is worse than one drawn in the wrong place.
    for (at, slot) in placed.iter_mut().enumerate() {
        if slot.is_none() {
            let column = next_column;
            next_column += 1.0;
            *slot = Some(Placed {
                at,
                depth: 0,
                across: column,
            });
        }
    }

    let mut nodes: Vec<Placed> = placed.into_iter().flatten().collect();
    nodes.sort_by(|a, b| {
        (a.depth, a.across, a.at)
            .partial_cmp(&(b.depth, b.across, b.at))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    nodes
}

/// The parent→child pairs to draw, as indices into the slice that was laid out.
///
/// Derived from the same `parent` field the layout uses, so an edge can never point somewhere the
/// layout did not place.
pub fn edges(experiments: &[Experiment]) -> Vec<(usize, usize)> {
    use std::collections::HashMap;
    let index: HashMap<&str, usize> = experiments
        .iter()
        .enumerate()
        .map(|(at, experiment)| (experiment.id.as_str(), at))
        .collect();
    experiments
        .iter()
        .enumerate()
        .filter_map(|(at, experiment)| {
            let parent = *index.get(experiment.parent.as_deref()?)?;
            (parent != at).then_some((parent, at))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real response from §247's probe: 5 experiments, 5 credits, one live run.
    fn probe() -> Value {
        serde_json::from_str(include_str!(
            "../tests/fixtures/autodiscovery-experiments.json"
        ))
        .expect("the captured experiments response")
    }

    /// The detail response for `node_2_0` from the same run, base64 clipped.
    fn detail() -> Value {
        serde_json::from_str(include_str!(
            "../tests/fixtures/autodiscovery-experiment-detail.json"
        ))
        .expect("the captured experiment detail")
    }

    #[test]
    fn the_probe_decodes_to_five_finished_experiments() {
        let experiments = decode_experiments(&probe());
        assert_eq!(experiments.len(), 5);
        assert!(finished(&probe()));
        assert!(experiments.iter().all(Experiment::succeeded));
        // Creation order, which is not the same as `id_in_run` order.
        assert_eq!(
            experiments.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["node_2_0", "node_2_1", "node_3_0", "node_3_1", "node_3_2"]
        );
        assert_eq!(
            experiments.iter().map(|e| e.number).collect::<Vec<_>>(),
            [1, 2, 3, 4, 5]
        );
    }

    /// §247's first correction: the sign of `surprise` is the direction, and the magnitude of the
    /// belief move is a *different number* — so deriving one from the other would be wrong twice.
    #[test]
    fn direction_comes_from_the_sign_of_surprise_not_from_the_belief_move() {
        let experiments = decode_experiments(&probe());
        let first = &experiments[0];
        assert_eq!(first.id, "node_2_0");

        let surprise = first.surprise.expect("a surprise score");
        assert!(surprise < 0.0, "{surprise}");
        assert_eq!(first.direction(), Direction::Away);
        assert_eq!(first.direction().label(), "Negative");

        // The two quantities genuinely disagree: the belief moved 0.360 and the score is 0.671.
        let prior = first.prior.expect("a prior").mean;
        let posterior = first.posterior.expect("a posterior").mean;
        let moved = (prior - posterior).abs();
        assert!(
            (moved - first.magnitude()).abs() > 0.2,
            "moved {moved}, reported {}",
            first.magnitude()
        );

        // And one that moved the other way is Positive.
        let up = experiments
            .iter()
            .find(|e| e.id == "node_3_1")
            .expect("node_3_1");
        assert!(up.surprise.expect("a score") > 0.0);
        assert_eq!(up.direction(), Direction::Toward);
        assert!(up.posterior.expect("posterior").mean > up.prior.expect("prior").mean);
    }

    /// §247's second correction: an experiment past the configured width was still not flagged.
    #[test]
    fn is_surprising_is_read_and_never_recomputed() {
        let experiments = decode_experiments(&probe());
        // The run was configured with `surprisal_width: 0.5`.
        let loudest = experiments
            .iter()
            .max_by(|a, b| a.magnitude().total_cmp(&b.magnitude()))
            .expect("an experiment");
        assert!(loudest.magnitude() > 0.5, "{}", loudest.magnitude());
        assert!(
            !loudest.surprising,
            "the server said false for a 0.67 shift at width 0.5 — the flag is not a threshold"
        );
        assert_eq!(experiments.iter().filter(|e| e.surprising).count(), 0);
    }

    /// §247's third correction: the label is the argmax of four vote counts, not a quartile.
    #[test]
    fn a_belief_label_comes_from_its_vote_counts() {
        let experiments = decode_experiments(&probe());
        let first = &experiments[0];

        // prior_belief: {definitely_true: 2, maybe_true: 3} — mean 0.7917, which as a quartile
        // would read `Likely True`. The votes say otherwise, and the votes win.
        let prior = first.prior.expect("a prior");
        assert!((prior.mean - 0.7917).abs() < 0.001, "{}", prior.mean);
        assert_eq!(prior.label, Confidence::MaybeTrue);
        assert_eq!(prior.label.label(), "Maybe True");

        // posterior_belief: all five votes on definitely_false, mean 0.4318 — a quartile would
        // read `Maybe False` and be understating it.
        let posterior = first.posterior.expect("a posterior");
        assert_eq!(posterior.label, Confidence::LikelyFalse);

        // An empty distribution falls back to the mean rather than claiming the first label.
        let empty = serde_json::json!({"mean": 0.9, "definitely_true": 0.0});
        assert_eq!(
            Belief::decode(Some(&empty)).expect("a belief").label,
            Confidence::LikelyTrue
        );
        // A tie leaves the less committed label standing.
        let tied = serde_json::json!({"mean": 0.6, "maybe_true": 2.0, "definitely_true": 2.0});
        assert_eq!(
            Belief::decode(Some(&tied)).expect("a belief").label,
            Confidence::MaybeTrue
        );
    }

    /// §247's fourth correction: every experiment in the probe descends from a node that is not in
    /// the response, so a dangling parent has to be the ordinary case.
    #[test]
    fn a_parent_outside_the_set_is_a_root() {
        let experiments = decode_experiments(&probe());
        let ids: Vec<&str> = experiments.iter().map(|e| e.id.as_str()).collect();
        assert!(
            !ids.contains(&"node_1_0"),
            "the probe's shared parent is genuinely absent"
        );
        assert_eq!(
            experiments[0].parent.as_deref(),
            Some("node_1_0"),
            "and it is genuinely referenced"
        );

        let placed = layout(&experiments);
        assert_eq!(placed.len(), 5, "nothing is dropped for a missing parent");
        // node_2_0 and node_2_1 both point at the absent node, so both are roots at depth 0.
        let depth_of = |id: &str| {
            let at = experiments.iter().position(|e| e.id == id).expect(id);
            placed.iter().find(|node| node.at == at).expect(id).depth
        };
        assert_eq!(depth_of("node_2_0"), 0);
        assert_eq!(depth_of("node_2_1"), 0);
        // And their children sit one below.
        assert_eq!(depth_of("node_3_0"), 1);
        assert_eq!(depth_of("node_3_2"), 1);
        assert_eq!(depth_of("node_3_1"), 1);
    }

    /// §247's fifth correction, and the one that would have cost a debugging round: the detail
    /// endpoint answers `child_ids` with an empty list for a node the list endpoint says has two.
    #[test]
    fn the_tree_is_built_from_parents_because_child_ids_disagrees_between_endpoints() {
        let listed = probe();
        let node = listed["experiments"]
            .as_array()
            .expect("experiments")
            .iter()
            .find(|item| item["experiment_id"] == "node_2_0")
            .expect("node_2_0 in the list");
        assert_eq!(
            node["child_ids"].as_array().map(Vec::len),
            Some(2),
            "the list knows both children"
        );

        let detailed = detail();
        assert_eq!(detailed["experiment"]["experiment_id"], "node_2_0");
        assert_eq!(
            detailed["experiment"]["child_ids"].as_array().map(Vec::len),
            Some(0),
            "and the detail endpoint says it has none"
        );
        // Same `parent_id` in both, which is why edges are built from that side.
        assert_eq!(
            detailed["experiment"]["parent_id"], node["parent_id"],
            "parents agree where children do not"
        );

        // The edges we draw: two from node_2_0, one from node_2_1, nothing dangling.
        let experiments = decode_experiments(&listed);
        let at = |id: &str| experiments.iter().position(|e| e.id == id).expect(id);
        let mut drawn = edges(&experiments);
        drawn.sort();
        let mut expected = vec![
            (at("node_2_0"), at("node_3_0")),
            (at("node_2_0"), at("node_3_2")),
            (at("node_2_1"), at("node_3_1")),
        ];
        expected.sort();
        assert_eq!(drawn, expected);
    }

    /// The list endpoint reports no figures even for an experiment that has one — so a count of
    /// zero here means "not fetched", never "none exist".
    #[test]
    fn figures_are_invisible_until_an_experiment_is_fetched_on_its_own() {
        let experiments = decode_experiments(&probe());
        assert!(
            experiments.iter().all(|e| e.figures == 0),
            "the list endpoint returns rich_outputs: null throughout"
        );

        // The detail response for the same experiment carries a full display bundle.
        let bundle = detail()["experiment"]["rich_outputs"]
            .as_array()
            .expect("rich_outputs in the detail response")
            .clone();
        assert_eq!(bundle.len(), 1);
        let mimes: Vec<&str> = bundle[0]
            .as_object()
            .expect("a display bundle")
            .keys()
            .map(String::as_str)
            .collect();
        for wanted in ["image/png", "image/jpeg", "image/svg+xml", "text/plain"] {
            assert!(mimes.contains(&wanted), "{wanted} missing from {mimes:?}");
        }
    }

    /// A tidy tree: leaves take columns in order, a parent sits over the middle of its children.
    #[test]
    fn a_parent_is_centred_over_its_children() {
        let experiments = decode_experiments(&probe());
        let placed = layout(&experiments);
        let across = |id: &str| {
            let at = experiments.iter().position(|e| e.id == id).expect(id);
            placed.iter().find(|node| node.at == at).expect(id).across
        };
        // node_2_0's two leaves take columns 0 and 1; it sits at 0.5 between them.
        let (left, right) = (across("node_3_0"), across("node_3_2"));
        assert!((across("node_2_0") - (left + right) / 2.0).abs() < 1e-9);
        // node_2_1 has one child and sits directly over it.
        assert_eq!(across("node_2_1"), across("node_3_1"));
        // Every column is distinct among the leaves.
        let mut columns = [left, right, across("node_3_1")];
        columns.sort_by(f64::total_cmp);
        assert_eq!(columns, [0.0, 1.0, 2.0]);
    }

    /// Nothing in the payload promises the parent chain is acyclic, and a hung window is a worse
    /// failure than a wrongly drawn one.
    #[test]
    fn a_cycle_terminates_and_still_places_every_node() {
        let cycle = serde_json::json!({
            "experiments": [
                {"experiment_id": "a", "parent_id": "b", "id_in_run": 1, "creation_idx": 1},
                {"experiment_id": "b", "parent_id": "a", "id_in_run": 2, "creation_idx": 2},
                {"experiment_id": "self", "parent_id": "self", "id_in_run": 3, "creation_idx": 3},
            ]
        });
        let experiments = decode_experiments(&cycle);
        assert_eq!(experiments.len(), 3);
        let placed = layout(&experiments);
        assert_eq!(placed.len(), 3, "every node is drawn somewhere");
        // A node that is its own parent is a root, not its own child.
        let at = experiments.iter().position(|e| e.id == "self").expect("self");
        assert_eq!(
            placed.iter().find(|node| node.at == at).expect("self").depth,
            0
        );
        assert!(!edges(&experiments).iter().any(|&(from, to)| from == to));
    }

    /// A run still working: rows exist, statuses are not terminal, and nothing panics on the gaps.
    #[test]
    fn a_half_finished_run_decodes_without_its_scores() {
        let partial = serde_json::json!({
            "has_job_completed": false,
            "experiments": [
                {"experiment_id": "node_1_0", "id_in_run": 1, "creation_idx": 1,
                 "status": "RUNNING", "hypothesis": "still going"},
                {"experiment_id": "  ", "id_in_run": 9},
            ]
        });
        let experiments = decode_experiments(&partial);
        assert!(!finished(&partial));
        assert_eq!(experiments.len(), 1, "a blank id cannot be an edge endpoint");
        let only = &experiments[0];
        assert!(!only.is_finished());
        assert!(!only.succeeded());
        assert_eq!(only.surprise, None);
        assert_eq!(only.magnitude(), 0.0);
        assert_eq!(only.direction(), Direction::Unmoved);
        assert_eq!(only.direction().label(), "—");
        assert!(only.prior.is_none());
    }

    /// The status vocabulary is open — §247 found `CREATED`, which the CLI's own icon table misses.
    #[test]
    fn an_unknown_status_is_treated_as_finished_rather_than_polled_forever() {
        let mut experiment = decode_experiments(&probe())[0].clone();
        for running in ["PENDING", "RUNNING", "IN_PROGRESS", "CREATED", ""] {
            experiment.status = running.to_string();
            assert!(!experiment.is_finished(), "{running}");
        }
        for stopped in ["SUCCEEDED", "COMPLETED", "FAILED", "ERROR", "CANCELLED", "WEDGED"] {
            experiment.status = stopped.to_string();
            assert!(experiment.is_finished(), "{stopped}");
        }
        experiment.status = "succeeded".into();
        assert!(experiment.succeeded(), "case is not load-bearing");
    }
}
