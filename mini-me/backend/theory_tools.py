"""Asta Theorizer submission tool + status polling for the hypothesis_generator.

The theorizer is a long (5–15 min, longer with novelty) async A2A job, and
Mini-Me is turn-driven — nothing runs between messages. So instead of blocking a
chat turn until the run finishes, `generate_theories` just *submits* the run and
returns immediately with a `task_id` and `status: "running"`. The subagent emits
a `running` HypothesisOutput (which populates the Theories card as a live
progress state), and the frontend polls the `/theorizer/{thread}/{task}` route —
which calls `poll_theory_status` here — until the theories are ready. The task id
lives in graph state, so a refresh just re-polls; no theories are ever lost and
the user never has to ask "is it done yet."

`poll_theory_status` runs `asta` inside the thread's sandbox (where the CLI is
authenticated via ``ASTA_TOKEN``), parses the task artifacts into the frontend
`HypothesisOutput` shape, and never invents theories.
"""

import json
import logging
import re
import shlex
from datetime import datetime, timezone
from typing import Any

from langchain_core.tools import tool

from backend.asta_jobs import _UUID_RE, _run, _state_of, fold_a2a_terminal_state, is_valid_task_id
from backend.runtime import _active_sandbox

logger = logging.getLogger(__name__)

_SUBMIT_TIMEOUT_S = 120
_POLL_TIMEOUT_S = 90


def _extract_json(output: str) -> dict[str, Any] | None:
    """Pull the JSON task record out of merged stdout/stderr execute output."""
    if not output:
        return None
    # aexecute appends stderr after a "[stderr]" marker; the record is on stdout.
    head = output.split("[stderr]", 1)[0].strip()
    for candidate in (head, output):
        try:
            return json.loads(candidate)
        except Exception:
            pass
        start, end = candidate.find("{"), candidate.rfind("}")
        if start != -1 and end > start:
            try:
                return json.loads(candidate[start : end + 1])
            except Exception:
                continue
    return None


def _paper_ref(entity: dict[str, Any]) -> dict[str, Any]:
    """Map an Asta paper entity to the frontend PaperRef shape (with a link)."""
    s2 = entity.get("s2Metadata", {}) or {}
    citation = entity.get("displayLabel") or s2.get("title") or "Untitled reference"
    ext = s2.get("externalIds") or {}
    doi = ext.get("DOI") if isinstance(ext, dict) else None
    arxiv = ext.get("ArXiv") if isinstance(ext, dict) else None
    corpus = s2.get("corpusId")
    if doi:
        url = f"https://doi.org/{doi}"
    elif arxiv:
        url = f"https://arxiv.org/abs/{arxiv}"
    elif s2.get("url"):
        url = s2.get("url")
    elif corpus is not None:
        # Theorizer papers usually carry ONLY a numeric corpusId (no DOI/url).
        # S2 paper *pages* are keyed by a 40-char hash, and the website's
        # /paper/CorpusID:<n> path resolves UNRELIABLY (it sent users to the
        # wrong paper). The API endpoint api.semanticscholar.org/CorpusID:<n>
        # 302-redirects to the correct canonical paper page — verified across
        # ids — so link through that instead.
        url = f"https://api.semanticscholar.org/CorpusID:{corpus}"
    else:
        url = None
    return {
        "citation": citation,
        "url": url,
        "doi": doi,
        "corpus_id": str(corpus) if corpus is not None else None,
    }


def _paper_ref_from_store(entry: dict[str, Any]) -> dict[str, Any]:
    """Map a theory_store ``paperstore`` entry to the frontend PaperRef shape.

    The store keeps the paper's identity at the top level (``title``,
    ``corpus_id``, ``s2_id``) and the Semantic Scholar record under
    ``s2_metadata`` (snake_case, unlike the legacy ``s2Metadata`` entity).
    Re-shaped into the legacy entity so `_paper_ref` stays the one place that
    decides how a paper is linked.
    """
    s2 = entry.get("s2_metadata") or {}
    if not isinstance(s2, dict):
        s2 = {}
    title = entry.get("title") or s2.get("title")
    corpus = entry.get("corpus_id")
    if corpus in (None, ""):
        corpus = s2.get("corpusId")
    return _paper_ref(
        {
            "displayLabel": title,
            "s2Metadata": {**s2, "title": title, "corpusId": corpus},
        }
    )


def _merge_patch(target: Any, patch: Any) -> Any:
    """Apply a JSON merge patch (RFC 7396): dicts merge recursively, ``null`` deletes.

    A theory_store artifact declares ``merge_format: json-merge-patch-rfc7396``
    and arrives as one part per patch — a paper, an extraction result, a theory
    — so the store is the fold of every part over an empty object.
    """
    if not isinstance(patch, dict):
        return patch
    if not isinstance(target, dict):
        target = {}
    for key, value in patch.items():
        if value is None:
            target.pop(key, None)
        else:
            target[key] = _merge_patch(target.get(key), value)
    return target


def _theory_store_of(task: dict[str, Any]) -> dict[str, Any]:
    """Fold every ``theory_store`` artifact's parts into one store dict."""
    store: dict[str, Any] = {}
    for art in task.get("artifacts", []) or []:
        if not isinstance(art, dict):
            continue
        if (art.get("metadata") or {}).get("type") != "theory_store":
            continue
        for part in art.get("parts", []) or []:
            data = part.get("data") if isinstance(part, dict) else None
            if isinstance(data, dict):
                store = _merge_patch(store, data)
    return store


_EVIDENCE_UUID_RE = re.compile(r"^e(\d+)(?:\.\d+)?$")


def _extraction_id_of(evidence_ref: Any) -> str | None:
    """``"e14.0"`` (a per-statement evidence uuid) -> ``"extraction-result-14"``.

    Theory statements cite evidence by the short uuid the extraction step
    stamped on each finding; the theory itself lists the full
    ``extraction-result-N`` ids. Both name the same extraction result.
    """
    if not isinstance(evidence_ref, str):
        return None
    if evidence_ref.startswith("extraction-result-"):
        return evidence_ref
    match = _EVIDENCE_UUID_RE.match(evidence_ref.strip())
    return f"extraction-result-{match.group(1)}" if match else None


def _papers_for_evidence(store: dict[str, Any], refs: list[Any]) -> list[dict[str, Any]]:
    """Resolve evidence refs -> extraction result -> paper store entry -> PaperRef.

    Order is the theory's own; a paper cited by several findings appears once.
    """
    extractions = store.get("extraction_results") or {}
    papers = store.get("paperstore") or {}
    seen: set[str] = set()
    refs_out: list[dict[str, Any]] = []
    for ref in refs:
        ext_id = _extraction_id_of(ref)
        ext = extractions.get(ext_id) if ext_id else None
        paper_id = ext.get("paper_id") if isinstance(ext, dict) else None
        if not paper_id or paper_id in seen:
            continue
        entry = papers.get(paper_id)
        if not isinstance(entry, dict):
            continue
        seen.add(paper_id)
        refs_out.append(_paper_ref_from_store(entry))
    return refs_out


def _theory_sort_key(theory_id: str) -> tuple[int, str]:
    match = re.search(r"(\d+)$", theory_id or "")
    return (int(match.group(1)) if match else 1 << 30, theory_id or "")


def _theories_from_store(store: dict[str, Any]) -> tuple[list[dict[str, Any]], list[str]]:
    """Theories + knowledge gaps from a folded theory_store.

    Each theory's laws are its ``theory_statements`` (the statement text, not
    the name); its papers come from ``supporting_evidence_ids``; papers the
    theorizer itself flags under ``conflicting_evidence`` become
    ``conflicting_papers``; and what it says the evidence leaves
    ``unaccounted_for`` is reported as a knowledge gap. Nothing is inferred
    from prose.
    """
    theories_in = store.get("theories") or {}
    if not isinstance(theories_in, dict):
        return [], []
    theories: list[dict[str, Any]] = []
    gaps: list[str] = []
    for theory_id in sorted(theories_in, key=_theory_sort_key):
        theory = theories_in[theory_id]
        if not isinstance(theory, dict):
            continue
        components = theory.get("components") or {}
        if not isinstance(components, dict):
            components = {}
        laws: list[str] = []
        novelty: float | None = None
        for statement in components.get("theory_statements") or []:
            law = statement.get("law") if isinstance(statement, dict) else None
            if not isinstance(law, dict):
                continue
            text = law.get("theory_statement") or law.get("statement_name")
            if isinstance(text, str) and text.strip():
                laws.append(text.strip())
            score = law.get("novelty_evaluation")
            if isinstance(score, (int, float)) and not isinstance(score, bool):
                novelty = float(score) if novelty is None else max(novelty, float(score))
        name = theory.get("name") or components.get("theory_name")
        if not laws and isinstance(name, str) and name.strip():
            laws = [name.strip()]
        conflicting_refs = [
            uuid
            for item in components.get("conflicting_evidence") or []
            if isinstance(item, dict)
            for uuid in (item.get("uuids") or [])
        ]
        for item in components.get("unaccounted_for") or []:
            text = item.get("text") if isinstance(item, dict) else None
            if isinstance(text, str) and text.strip() and text.strip() not in gaps:
                gaps.append(text.strip())
        existing = components.get("existing_theory")
        if not isinstance(existing, dict):
            existing = {}
        theories.append(
            {
                "name": name.strip() if isinstance(name, str) and name.strip() else None,
                "summary": _clean_text(components.get("theory_description_plain_language")),
                "laws": laws,
                "supporting_papers": _papers_for_evidence(
                    store, list(theory.get("supporting_evidence_ids") or [])
                ),
                "conflicting_papers": _papers_for_evidence(store, conflicting_refs),
                "novelty_score": novelty,
                # The theorizer's own, free verdict on how the theory sits against
                # the canon — e.g. "somewhat-related-to-existing" — and the classic
                # work it names. Not the paid novelty evaluation, which scores laws.
                "novelty_label": _clean_text(existing.get("likely_classification")),
                "related_work": _clean_list(existing.get("references")),
                "predictions": _clean_list(components.get("new_predictions_likely")),
                "negative_experiments": _clean_list(components.get("negative_experiments")),
            }
        )
    return theories, gaps


def _clean_text(value: Any) -> str | None:
    return value.strip() if isinstance(value, str) and value.strip() else None


def _clean_list(values: Any) -> list[str]:
    if not isinstance(values, list):
        return []
    return [v.strip() for v in values if isinstance(v, str) and v.strip()]


def _laws_from_content(content: list[dict[str, Any]]) -> list[str]:
    """Extract theory statements from the 'Theory Statements' content container."""
    by_id = {c.get("id"): c for c in content}
    container = next(
        (
            c
            for c in content
            if c.get("type") == "SECTIONS"
            and "theory statement" in (c.get("title") or "").lower()
        ),
        None,
    )
    laws: list[str] = []
    if container:
        for cid in container.get("childIds") or []:
            node = by_id.get(cid)
            if node and node.get("title"):
                laws.append(node["title"].strip())
    return laws


def _parse_theories(task: dict[str, Any]) -> dict[str, Any]:
    """Parse a completed Asta task record into HypothesisOutput field values.

    Two record shapes are read, because the Theorizer changed its output
    without a version bump: the legacy one emitted one ``theory`` artifact per
    theory (a content tree with a "Theory Statements" section and ``entities``
    for the papers); the current one emits a single ``theory_store`` artifact —
    a sequence of JSON merge patches carrying the paper store, the extraction
    results and the theories, which the paper-finding and extraction stages
    also write to. The deployed bug was reading only the first: a completed run
    of eight theories came back as a completed run of none.
    """
    theories: list[dict[str, Any]] = []
    paper_ids: set[str] = set()
    for art in task.get("artifacts", []) or []:
        if not isinstance(art, dict):
            continue
        if art.get("metadata", {}).get("type") != "theory":
            continue
        try:
            data = art["parts"][0]["data"]
        except (KeyError, IndexError, TypeError):
            continue
        laws = _laws_from_content(data.get("content", []) or [])
        if not laws and data.get("name"):
            laws = [data["name"]]
        supporting = []
        for pid, ent in (data.get("entities", {}) or {}).items():
            paper_ids.add(pid)
            supporting.append(_paper_ref(ent))
        theories.append(
            {
                "name": data.get("name") or None,
                "summary": None,
                "laws": laws,
                "supporting_papers": supporting,
                # The legacy record surfaces conflicting evidence as prose, not
                # as a per-paper split, so we do not fabricate a partition here.
                "conflicting_papers": [],
                "novelty_score": None,
                "novelty_label": None,
                "related_work": [],
                "predictions": [],
                "negative_experiments": [],
            }
        )
    store = _theory_store_of(task)
    store_theories, gaps = _theories_from_store(store)
    theories.extend(store_theories)
    # "Papers reviewed" is what the pipeline extracted from. The store says so
    # directly (one extraction result per paper read); the legacy record only
    # tells us which papers a theory ended up citing.
    extractions = store.get("extraction_results") or {}
    if isinstance(extractions, dict) and extractions:
        reviewed = {
            ext.get("paper_id")
            for ext in extractions.values()
            if isinstance(ext, dict) and ext.get("paper_id")
        }
        papers_reviewed = len(reviewed)
    else:
        for theory in store_theories:
            for paper in theory["supporting_papers"]:
                paper_ids.add(paper.get("corpus_id") or paper.get("citation"))
        papers_reviewed = len(paper_ids)
    return {
        "theories": theories,
        "knowledge_gaps": gaps,
        "papers_reviewed": papers_reviewed,
    }


def _progress_of(task: dict[str, Any] | None) -> str:
    if not task:
        return ""
    try:
        parts = task["status"]["message"]["parts"]
        return parts[0]["data"].get("short_desc", "") or ""
    except Exception:
        return ""


def _run_elapsed_seconds(task: dict[str, Any] | None) -> int | None:
    """Seconds since the task actually started, from its `started_at` timestamp."""
    if not task:
        return None
    try:
        started = task["status"]["message"]["parts"][0]["data"].get("started_at")
        if not started:
            return None
        dt = datetime.fromisoformat(started)
        return int((datetime.now(timezone.utc) - dt).total_seconds())
    except Exception:
        return None


# In-sandbox reducer for `asta generate-theories task <id>`. A completed task
# record is ~500 KB — it embeds the full paper store and per-paper extraction
# markdown — which blows past the sandbox execute truncation cap and lands as
# unparseable JSON (the bug that left the Theories card "generating" forever).
# We only need the task state and the theories, so prune everything else
# (extraction schemas, per-paper markdown, extracted data) and trim each
# theory to the fields the client-side parser reads BEFORE the payload
# crosses back. Legacy `theory` artifacts keep name, section titles and paper
# refs; a `theory_store` artifact (the current shape: merge patches holding
# the paper store, extraction results and theories together) is folded and
# cut down to statements, evidence ids, and the paper identities those ids
# resolve to. The output is the same reduced-task shape
# `_parse_theories`/`_state_of` already consume, so the parser stays the single
# source of truth. Kept apostrophe-free so it survives shell single-quoting.
_REDUCE_TASK_PY = """
import sys, json
try:
    t = json.load(sys.stdin)
except Exception:
    sys.exit(0)
st = t.get("status", {}) or {}
def te(e):
    s = e.get("s2Metadata", {}) or {}
    return {"displayLabel": e.get("displayLabel"),
            "s2Metadata": {"title": s.get("title"), "corpusId": s.get("corpusId"),
                           "externalIds": s.get("externalIds"), "url": s.get("url")}}
def tc(c):
    return [{"id": n.get("id"), "type": n.get("type"), "title": n.get("title"),
             "childIds": n.get("childIds")}
            for n in c if n.get("type") in ("SECTIONS", "SECTION")]
arts = []
for a in t.get("artifacts", []) or []:
    if (a.get("metadata") or {}).get("type") != "theory":
        continue
    try:
        d = a["parts"][0]["data"]
    except Exception:
        continue
    arts.append({"metadata": {"type": "theory"}, "parts": [{"data": {
        "name": d.get("name"),
        "content": tc(d.get("content", []) or []),
        "entities": {k: te(v) for k, v in (d.get("entities", {}) or {}).items()},
    }}]})
def mp(t, p):
    if not isinstance(p, dict):
        return p
    if not isinstance(t, dict):
        t = {}
    for k, v in p.items():
        if v is None:
            t.pop(k, None)
        else:
            t[k] = mp(t.get(k), v)
    return t
store = {}
for a in t.get("artifacts", []) or []:
    if (a.get("metadata") or {}).get("type") != "theory_store":
        continue
    for p in a.get("parts", []) or []:
        d = p.get("data") if isinstance(p, dict) else None
        if isinstance(d, dict):
            store = mp(store, d)
ths = store.get("theories")
if isinstance(ths, dict) and ths:
    def ts(s):
        law = (s.get("law") or {}) if isinstance(s, dict) else {}
        nov = law.get("novelty_evaluation")
        if isinstance(nov, bool) or not isinstance(nov, (int, float)):
            nov = None
        return {"law": {"statement_name": law.get("statement_name"),
                        "theory_statement": law.get("theory_statement"),
                        "novelty_evaluation": nov}}
    def tv(items):
        return [{"text": i.get("text"), "uuids": i.get("uuids")}
                for i in (items or []) if isinstance(i, dict)]
    def tt(th):
        c = th.get("components") or {}
        if not isinstance(c, dict):
            c = {}
        ex = c.get("existing_theory")
        if not isinstance(ex, dict):
            ex = {}
        return {"id": th.get("id"), "name": th.get("name"),
                "supporting_evidence_ids": th.get("supporting_evidence_ids"),
                "components": {
                    "theory_name": c.get("theory_name"),
                    "theory_description_plain_language": c.get("theory_description_plain_language"),
                    "theory_statements": [ts(s) for s in (c.get("theory_statements") or [])],
                    "conflicting_evidence": tv(c.get("conflicting_evidence")),
                    "unaccounted_for": tv(c.get("unaccounted_for")),
                    "new_predictions_likely": c.get("new_predictions_likely"),
                    "negative_experiments": c.get("negative_experiments"),
                    "existing_theory": {"likely_classification": ex.get("likely_classification"),
                                        "references": ex.get("references")}}}
    def tp(e):
        s = e.get("s2_metadata") or {}
        if not isinstance(s, dict):
            s = {}
        return {"id": e.get("id"), "title": e.get("title"), "corpus_id": e.get("corpus_id"),
                "s2_id": e.get("s2_id"),
                "s2_metadata": {"title": s.get("title"), "corpusId": s.get("corpusId"),
                                "externalIds": s.get("externalIds"), "url": s.get("url")}}
    ex = store.get("extraction_results") or {}
    ps = store.get("paperstore") or {}
    arts.append({"metadata": {"type": "theory_store"}, "parts": [{"data": {
        "theories": {k: tt(v) for k, v in ths.items() if isinstance(v, dict)},
        "extraction_results": {k: {"paper_id": v.get("paper_id")}
                               for k, v in ex.items() if isinstance(v, dict)},
        "paperstore": {k: tp(v) for k, v in ps.items() if isinstance(v, dict)},
    }}]})
out = {"status": st, "artifacts": arts}
if st.get("state") != "completed":
    # A failed/canceled record has no paper store, so forwarding more of it stays
    # small. Two things help diagnose a failure the client-side parser otherwise
    # cannot: (1) top-level error hints on builds that populate them (status.message
    # is often null — verified against a real failed run), and (2) the TYPES of
    # artifacts the run produced before dying, which distinguish a mid-run crash
    # (built an extraction-schema/theory_store, no theories) from one that never
    # started. We deliberately do NOT forward the history text: on a real failure
    # its tail was a benign "Task accepted", which would masquerade as a reason.
    ef = {}
    for k in ("error", "detail", "reason", "message"):
        v = t.get(k)
        if isinstance(v, str) and v.strip():
            ef[k] = v
    if ef:
        out["error_fields"] = ef
    types = []
    for a in (t.get("artifacts") or []):
        ty = (a.get("metadata") or {}).get("type")
        if ty and ty not in types:
            types.append(ty)
    if types:
        out["artifact_types"] = types
print(json.dumps(out))
"""


def _poll_command(task_id: str) -> str:
    """Shell command: fetch the task and reduce it in-sandbox to a small record.

    asta streams progress to stderr and the task JSON to stdout, so drop stderr
    and pipe stdout through the reducer. `python3` ships with the sandbox image
    (asta itself is a Python CLI).
    """
    fetch = f"asta generate-theories task {shlex.quote(task_id)} 2>/dev/null"
    return f"{fetch} | python3 -c {shlex.quote(_REDUCE_TASK_PY)}"


def _build_submit_command(question: str, max_papers: int, do_novelty: bool) -> list[str]:
    """Build the argv for submitting a theorizer run (asta CLI v0.101.0).

    Kept pure (no I/O) so the CLI contract is unit-testable — the deployed bug
    was the agent hand-building this with a non-existent ``--question`` flag.
    The correct subcommand is ``literature-theory-generation`` and the query
    flag is ``--theory-query``.
    """
    args = [
        "asta",
        "generate-theories",
        "literature-theory-generation",
        "--theory-query",
        question,
        "--max-papers-to-retrieve",
        str(max_papers),
    ]
    if not do_novelty:
        # Novelty eval adds 30–60 min; off unless explicitly requested.
        args.append("--no-do-qualified-novelty-evaluation")
    # Return the task id immediately instead of blocking the CLI on the run.
    args.append("--no-wait")
    return args


async def _submit(sandbox: Any, question: str, max_papers: int, do_novelty: bool) -> str | None:
    args = _build_submit_command(question, max_papers, do_novelty)
    cmd = " ".join(shlex.quote(a) for a in args)
    out = await _run(sandbox, cmd, _SUBMIT_TIMEOUT_S)
    match = _UUID_RE.search(out)
    return match.group(0) if match else None


def _failure_reason(task: dict[str, Any] | None, state: str) -> str:
    """Best-effort human-readable reason a theorizer task ended non-successfully.

    A2A carries the failure detail in ``status.message``; different builds put it
    under ``short_desc``, a plain ``text`` part, or a free-form ``data`` blob. We
    were only reading ``short_desc``, so real errors surfaced as the useless
    "Theorizer task failed." Probe the likely spots and fall back to a generic.
    """
    message = ((task or {}).get("status") or {}).get("message")
    # Some builds set status.message to a plain string instead of a parts list.
    if isinstance(message, str) and message.strip():
        return message.strip()
    parts = message.get("parts") if isinstance(message, dict) else None
    for part in parts or []:
        if not isinstance(part, dict):
            continue
        text = part.get("text")
        if isinstance(text, str) and text.strip():
            return text.strip()
        data = part.get("data")
        if isinstance(data, dict):
            for key in ("short_desc", "error", "message", "detail", "reason", "status"):
                val = data.get(key)
                if isinstance(val, str) and val.strip():
                    return val.strip()
    # Top-level error the reducer carries through for non-completed tasks (some
    # builds put the detail here instead of status.message).
    if isinstance(task, dict):
        ef = task.get("error_fields")
        if isinstance(ef, dict):
            for key in ("reason", "error", "detail", "message"):
                val = ef.get(key)
                if isinstance(val, str) and val.strip():
                    return val.strip()
    # No machine-readable reason anywhere: Asta commonly fails a run with a bare
    # status ({state, timestamp}) — verified against a real failed run. Be honest
    # rather than inventing detail, and note how far the run got (artifact types
    # the reducer forwarded) so the user can tell a mid-run crash from one that
    # never started, and knows a retry is the right move.
    reason = f"Asta ended the run as {state} without reporting a reason"
    types = task.get("artifact_types") if isinstance(task, dict) else None
    if isinstance(types, list) and types and "theory" not in types:
        reason += f" (it produced intermediate artifacts — {', '.join(types)} — but no theories)"
    return (
        reason + ". The run failed inside Asta's own service, not Mini-Me. Retry "
        "once; if it keeps failing, the Asta Theorizer is unavailable for your "
        "account right now (independent of the model or query)."
    )


async def poll_theory_status(sandbox: Any, task_id: str) -> dict[str, Any]:
    """One poll of a theorizer task; parse theories when it has completed.

    Returns a dict shaped for the frontend:
      completed -> {"status":"completed","theories":[...],"knowledge_gaps":[],"papers_reviewed":N}
      running   -> {"status":"running","task_id":...,"elapsed_seconds":...,"progress":...}
      failed    -> {"status":"failed"|"canceled","task_id":...,"reason":...}
    """
    out = await _run(sandbox, _poll_command(task_id), _POLL_TIMEOUT_S)
    task = _extract_json(out)
    state = _state_of(task)
    if state == "completed" and task is not None:
        return {"status": "completed", "task_id": task_id, **_parse_theories(task)}
    if (status := fold_a2a_terminal_state(state)) is not None:
        reason = _failure_reason(task, state)
        logger.warning("theorizer task %s ended %s: %s", task_id, state, reason)
        return {
            "status": status,
            "task_id": task_id,
            "reason": reason,
        }
    return {
        "status": "running",
        "task_id": task_id,
        "elapsed_seconds": _run_elapsed_seconds(task),
        "progress": _progress_of(task),
    }


def theories_markdown(question: str, result: dict[str, Any]) -> str:
    """Render a completed poll result as a readable, self-contained theories doc.

    Written to the sandbox so the agent (which has filesystem tools) can read
    and reason over the theories on a later turn, and so the run is a durable,
    downloadable artifact instead of living only in the frontend card.
    """
    theories = result.get("theories") or []
    reviewed = result.get("papers_reviewed") or 0
    lines = [
        f"# Theories: {question}".rstrip(),
        "",
        f"_Generated by the Asta Theorizer — {len(theories)} "
        f"theor{'y' if len(theories) == 1 else 'ies'}, {reviewed} papers reviewed. "
        "AI-generated; review with a subject-matter expert before use._",
        "",
    ]
    for i, theory in enumerate(theories, 1):
        laws = theory.get("laws") or []
        name = theory.get("name")
        lines.append(f"## Theory {i}: {name}" if name else f"## Theory {i}")
        if theory.get("summary"):
            lines.append(f"_{theory['summary']}_")
            lines.append("")
        for law in laws:
            lines.append(f"- {law}")
        label = theory.get("novelty_label")
        related = theory.get("related_work") or []
        if label or related:
            lines.append("")
            lines.append(
                f"**Against existing theory:** {label.replace('-', ' ')}" if label
                else "**Related existing theory**"
            )
            lines.extend(f"- {ref}" for ref in related)
        for heading, key in (
            ("**Predictions**", "predictions"),
            ("**Experiments that would refute it**", "negative_experiments"),
        ):
            items = theory.get(key) or []
            if items:
                lines.append("")
                lines.append(heading)
                lines.extend(f"- {item}" for item in items)
        for heading, key in (
            ("**Supporting papers**", "supporting_papers"),
            ("**Conflicting or complicating evidence**", "conflicting_papers"),
        ):
            papers = theory.get(key) or []
            if not papers:
                continue
            lines.append("")
            lines.append(heading)
            for paper in papers:
                citation = paper.get("citation") or "Untitled reference"
                url = paper.get("url")
                lines.append(f"- [{citation}]({url})" if url else f"- {citation}")
        lines.append("")
    gaps = result.get("knowledge_gaps") or []
    if gaps:
        lines.append("## Knowledge gaps")
        lines.extend(f"- {gap}" for gap in gaps)
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


async def persist_theory_outputs(
    sandbox: Any, task_id: str, question: str, result: dict[str, Any]
) -> list[str]:
    """Write a terminal poll result to the sandbox as durable, agent-readable files.

    Completed runs get ``theories/<task_id>.md`` (readable) + ``.json``
    (structured); failures get ``theories/<task_id>.error.log`` carrying the real
    reason. Files land under a NON-hidden dir so ``FileSyncMiddleware`` surfaces
    them as artifacts on the next run and the agent can ``read_file`` them.
    Best-effort: a write failure is logged, never raised into the poll response.
    """
    awrite = getattr(sandbox, "awrite", None)
    if awrite is None:
        return []
    try:
        work_dir = await sandbox.aget_work_dir()
    except Exception:
        work_dir = "/workspace"
    base = f"{work_dir}/theories/{task_id}"
    status = result.get("status")
    written: list[str] = []
    try:
        if status == "completed":
            targets = {
                f"{base}.md": theories_markdown(question, result),
                f"{base}.json": json.dumps(
                    {"question": question, **result}, indent=2, ensure_ascii=False
                ),
            }
        else:
            reason = result.get("reason") or f"Theorizer task {status}."
            targets = {
                f"{base}.error.log": (
                    f"task_id: {task_id}\n"
                    f"question: {question}\n"
                    f"status: {status}\n"
                    f"reason: {reason}\n"
                )
            }
        for path, content in targets.items():
            res = await awrite(path, content)
            err = getattr(res, "error", None)
            if err:
                logger.warning("failed to persist %s: %s", path, err)
            else:
                written.append(path)
    except Exception as exc:  # noqa: BLE001
        logger.warning("persist_theory_outputs failed for %s: %s", task_id, exc)
    return written


@tool
async def generate_theories(
    question: str = "",
    resume_task_id: str = "",
    max_papers: int = 30,
    do_novelty: bool = False,
) -> str:
    """Start a literature-grounded theory-generation run (Asta Theorizer).

    Call this once with the research `question`. It submits the run and returns
    immediately with `status: "running"` and a `task_id` — it does NOT block until
    the theories are ready. The Theories panel then fills in on its own as the run
    completes (usually 5–15 minutes); the user does not need to ask.

    When you get `status: "running"`, return a HypothesisOutput with
    `status="running"`, that `task_id`, the user's `question`, and empty
    `theories`, and tell the user their theories are being generated and will
    appear in the Theories panel automatically. Do NOT call this tool again in a
    loop — the frontend watches the task.

    Only pass `resume_task_id` if the user explicitly asks you to check on a
    specific run; it does a single status poll and returns the theories if ready.

    Args:
        question: the research question to theorize about (required to start).
        resume_task_id: an existing task id to poll once (explicit check only).
        max_papers: how many papers to retrieve (20–30 keeps runs faster).
        do_novelty: run qualified-novelty evaluation. Adds 30–60 minutes and is
            OFF by default. Only set it True when the user explicitly asks to run
            a novelty evaluation and accepts the long wait — a question that merely
            mentions "novelty" or asks for a novelty score is NOT such a request.

    Returns:
        JSON string. Starting a run -> {"status":"running","task_id":...,...}.
        Explicit check -> completed/running/failed per `poll_theory_status`.
        Errors -> {"status":"error","message":...}.
    """
    try:
        sandbox = _active_sandbox.get()
    except LookupError:
        sandbox = None
    if sandbox is None:
        return json.dumps(
            {"status": "error", "message": "No active sandbox; cannot run the theorizer."}
        )

    resume_task_id = resume_task_id.strip()
    if resume_task_id:
        if not is_valid_task_id(resume_task_id):
            return json.dumps({"status": "error", "message": "Invalid task id."})
        try:
            return json.dumps(await poll_theory_status(sandbox, resume_task_id))
        except Exception as exc:  # noqa: BLE001
            return json.dumps({"status": "error", "message": f"poll failed: {exc}"})

    if not question.strip():
        return json.dumps(
            {"status": "error", "message": "A research question is required to start a run."}
        )
    try:
        task_id = await _submit(sandbox, question.strip(), max_papers, do_novelty)
    except Exception as exc:  # noqa: BLE001
        return json.dumps({"status": "error", "message": f"submit failed: {exc}"})
    if not task_id:
        return json.dumps(
            {
                "status": "error",
                "message": (
                    "The Asta theorizer returned no task id. This usually means "
                    "your Asta access token is missing or expired — refresh it in "
                    "Settings → Asta connection (run `asta auth login` then "
                    "`asta auth print-token`), then try again."
                ),
            }
        )

    return json.dumps(
        {
            "status": "running",
            "task_id": task_id,
            "question": question.strip(),
            "note": "Theory generation started; it runs in the background (usually 5–15 min).",
            "instruction": (
                "Return a HypothesisOutput now with status='running', task_id='"
                f"{task_id}', the user's question, and empty theories. Tell the user "
                "their theories are being generated and will appear in the Theories "
                "panel automatically in a few minutes. Do NOT call this tool again."
            ),
        }
    )
