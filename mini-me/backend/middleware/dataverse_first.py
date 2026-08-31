"""Make the dataverse explorer search, and read what it found, before it recommends anything.

# Why this one is next

`dataverse_explorer` carries ``response_format=DataVerseSearchResults``, so it has the exit set
out in `middleware/tool_gate.py`: its first model call is forced, and one of the things it may
call is the schema itself, answering the whole question from memory in a single step.

What comes back when it does is a list of `DataVerseFindings`, and the required field on each is
``persistent_id`` — *"Dataset DOI or persistent identifier"*. **A persistent id composed from
memory is a citation a researcher will paste into a paper without checking**, exactly as they
clicked the DOIs that started this. It is the same failure as `academic_researcher`'s with a
shorter fuse, and unlike a plausible-looking reference, a wrong `persistent_id` is not something a
reader can catch by recognising the title.

# Two steps, because one would not be enough

`SearchCIPDataverse` writes its results to a **file**; `read_search_results` is what puts the
metadata in front of the model. So a gate that opened as soon as a search returned would let the
subagent search, satisfy the gate, and then still compose every field from memory — having proven
only that it can call a tool.

The workflow the skill documents is search → read → recommend
(`skills/dataverse/references/discovery_workflow.md`), and both of the first two are forced here.
`list_dataset_files` is step three in that document and is *not* forced: it is for shortlisted
datasets only, it is a judgement about how much detail a recommendation needs, and nothing in the
schema depends on it.

# The handoff is made, not requested

The prompt carried this:

    Mandatory fixed filename rule: ALWAYS call `SearchCIPDataverse` with
    `output_filename="dataverse_search.json"` and ALWAYS call `read_search_results` with
    `filename="dataverse_search.json"`. Do not invent or vary this name.

That is a mechanical fact about two tools that have to agree, written in capital letters and handed
to a model to remember across a multi-step episode.

**Both the prompt and the middleware that replaced it named an argument that does not exist.**
`read_search_results` takes `file_path`, and it wants the *server-side absolute path* — not a bare
name. Probed against the live MCP (docs §220):

    read_search_results(filename=...)                    -> 'file_path' is a required property
    read_search_results(file_path=..., filename=...)     -> Unexpected keyword argument
    read_search_results(file_path="/tmp/mcp/json_files/dataverse_search.json")  -> the metadata

So injecting `filename` did not merely fail to help: it made every read fail, including the reads
the gate above forces. `dataverse_explorer` could search and could never read, which is what the
researcher saw — nine steps, ninety seconds, and *"couldn't extract parseable metadata."*

The path is taken from the search's own answer (`{"output_file": "/tmp/mcp/json_files/..."}`)
rather than assumed, so a server that moves its directory is followed rather than guessed at.

# The results are kept where the researcher can open them

That file lives on the MCP host — `/tmp/mcp/json_files/` on a machine at
`dataverse-cip.fastmcp.app`, which is nobody's workspace. *"I want the user to have it."* So what
comes back from the read is written into the sandbox as `dataverse_search.json`, where
`FileSyncMiddleware` surfaces it in Outputs and `middleware/claims.py` can check the recommended
`persistent_id`s against it. Until this, that check was reading a path that never existed.
"""

from __future__ import annotations

import json
import logging
import re
from collections.abc import Awaitable, Callable
from typing import Any

from langchain.agents.middleware import AgentMiddleware

from backend import mcp_tools
from backend.middleware.tool_gate import Step, ToolsBeforeAnswering

logger = logging.getLogger(__name__)

#: The tool that queries CIP Dataverse and writes its results to disk.
SEARCH_TOOL = "SearchCIPDataverse"

#: The tool that reads those results back. Until this has returned, the model has a file it has
#: never seen the contents of.
READ_TOOL = "read_search_results"

#: The one name both tools must agree on.
#:
#: **On the MCP host, successive searches overwrite it, and that is right**: there it is a hand-off
#: between two calls. The *workspace* copy is a different thing wearing the same name — it is what
#: the researcher opens and what `middleware/claims.py` checks recommendations against — and
#: overwriting that one was a defect.
#:
#: A real turn ran forty-six steps and several searches. The explorer recommended a dataset it had
#: found early, `doi:10.21223/J9NLVP` — real, published, with real authors — and the claims check
#: compared it against the *last* search's results, which no longer contained it:
#:
#:     WARNING claims: dataverse_explorer recommended 1 datasets, 1 absent from
#:     dataverse_search.json: doi:10.21223/J9NLVP
#:
#: A false accusation of the one thing that module exists to catch, and its own docstring names the
#: cost: *"a record that cries wolf once is a record nobody reads the second time"*. The workspace
#: copy now accumulates across the turn (§286).
FIXED_FILENAME = "dataverse_search.json"

#: What the searches themselves reported, beside the records they returned.
#:
#: Its own file rather than a wrapper around the array, because the array is what the panel
#: decodes, what `claims.unsearched` walks and what a researcher opens — three readers that would
#: all have to learn a new shape to carry one number. Under `.mini-me/` so `workspace::outputs`
#: does not list it as something the research produced (§300).
SEARCH_META_DIR = ".mini-me"
SEARCH_META_NAME = "dataverse_search.meta.json"

#: The sentence `mcp_tools._save_mcp_to_sandbox` hands the model instead of a large answer.
#:
#: **This is what `_keep` was trying to parse.** A result over `MCP_TOOL_OUTPUT_MAX_BYTES`
#: (128 KB) is written to the workspace and the model receives a pointer with a 2 KB preview —
#: which is prose, so `json.loads` fails, `_payload` answers `None`, and `_keep` returned in
#: silence. A search returning 100 datasets produced no file and no log line; a narrow one
#: returning four kept its four. That is why the panel said *1 dataset found* against a 314 KB
#: answer (§291).
SAVED_POINTER = re.compile(r"saved to `([^`]+)`")

#: The marker `_truncate_str_result` splices into a capped answer.
#:
#: Distinct from the pointer: this is the case where the full text was **not** saved anywhere, so
#: there is nothing to follow and the only honest thing is to say so.
TRUNCATION_MARKER = "[output truncated"

#: How many records the workspace copy will hold before it stops growing.
#:
#: A model that loops on searches must not fill a researcher's disk, and a file nobody can open is
#: not a record. Generous, because the whole point is that a recommendation made forty steps ago is
#: still checkable.
MAX_KEPT_RECORDS = 2_000

#: Where the MCP writes when it is not told otherwise, used only if a read is somehow reached
#: without a search having answered first. The gate above makes that ordering hard to produce, and
#: a stale constant is still better than no argument at all.
DEFAULT_SERVER_DIR = "/tmp/mcp/json_files"


#: Where each field of a rendered row can be found, in order of preference.
#:
#: **Declared, and tried in order, because the MCP's layout is not ours.** `_ids_in` states the
#: rule this follows: a reader that insisted on one key *"would report every dataset as fabricated
#: the day that layout changed"*. So each field names every spelling seen or documented, an unknown
#: record yields empty strings rather than an exception, and the original is kept beside the
#: normalised form so nothing is lost when a name we have never met turns up.
FIELDS: dict[str, tuple[str, ...]] = {
    "persistent_id": ("global_id", "persistentId", "persistent_id", "doi", "identifier"),
    "title": ("name", "title", "label"),
    "link": ("url", "persistentUrl", "link", "href"),
    "description": ("description", "dsDescription", "abstract", "summary"),
    "repository": ("name_of_dataverse", "publisher", "identifier_of_dataverse", "repository"),
}

#: Fields whose value is a count.
COUNT_FIELDS: tuple[str, ...] = ("fileCount", "file_count", "files_count")

#: Fields whose value is a list of people.
AUTHOR_FIELDS: tuple[str, ...] = ("authors", "author", "creators", "creator")


def _first_string(record: dict[str, Any], keys: tuple[str, ...]) -> str:
    for key in keys:
        value = record.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
    return ""


def _joined_identifier(record: dict[str, Any]) -> str:
    """`doi:10.21223/P3/X` rebuilt from a record that stores its parts separately.

    Dataverse's native representation splits `protocol` / `authority` / `identifier`, so the id a
    researcher cites appears nowhere in it as a single string — which is also how a recommendation
    can look absent from a search that returned it (§288).
    """
    protocol = _first_string(record, ("protocol",))
    authority = _first_string(record, ("authority",))
    identifier = _first_string(record, ("identifier",))
    if protocol and authority and identifier:
        return f"{protocol}:{authority}/{identifier}"
    return ""


def normalise(record: Any) -> dict[str, Any]:
    """One search result in the shape the app renders.

    **This is what takes the model out of the citation business.** The datasets panel used to
    render `DataVerseSearchResults.datasets` — seven fields per row, every one retyped by a model
    out of a file it had just read — and on a real turn six of six `persistent_id`s were composed
    rather than copied (§289). A row built here comes from the API's own answer, so a fabricated
    identifier has no row to appear in.

    The original is carried under `raw`, both because a field we failed to map is not a field
    worth losing, and because `claims.unsearched` walks the leaves of this file: an id that only
    exists under a name we have never met is still findable there.
    """
    if not isinstance(record, dict):
        return {"persistent_id": "", "title": str(record), "raw": record}

    authors: list[str] = []
    for key in AUTHOR_FIELDS:
        value = record.get(key)
        if isinstance(value, list):
            authors = [str(item).strip() for item in value if str(item).strip()]
            break
        if isinstance(value, str) and value.strip():
            authors = [value.strip()]
            break

    file_count: int | None = None
    for key in COUNT_FIELDS:
        value = record.get(key)
        if isinstance(value, bool):
            continue
        if isinstance(value, int):
            file_count = value
            break
        if isinstance(value, str) and value.strip().isdigit():
            file_count = int(value.strip())
            break

    return {
        # **Joined form first.** `identifier` is a candidate key and on a split record it holds
        # `P3/HKABUV` alone — which is not a persistent id, and taking it would leave the row
        # carrying half a citation. If the parts are all present they win.
        "persistent_id": _joined_identifier(record)
        or _first_string(record, FIELDS["persistent_id"]),
        "title": _first_string(record, FIELDS["title"]),
        "link": _first_string(record, FIELDS["link"]),
        "description": _first_string(record, FIELDS["description"]),
        "authors": authors,
        "file_count": file_count,
        "repository": _first_string(record, FIELDS["repository"]),
        # Kept whole. See the docstring: an unmapped field is not a field worth losing.
        "raw": record,
    }


class SearchBeforeRecommending(ToolsBeforeAnswering):
    """Force a Dataverse search, then a read of it, before recommendations become reachable."""

    steps = (
        Step(
            force=SEARCH_TOOL,
            because="dataverse_explorer has not searched CIP Dataverse yet",
        ),
        Step(
            force=READ_TOOL,
            because="dataverse_explorer has not read what its search returned",
        ),
    )


class SearchResultsFile(AgentMiddleware):
    """Make the two Dataverse tools agree on one file, and keep a copy the researcher can open.

    Three things, all mechanical, none of them a judgement a model should be making mid-episode:

    * the search is told where to write (`output_filename`);
    * the read is told where to look (`file_path`), taken from what the search answered;
    * what the read returns is saved into the workspace, because the file itself is on the MCP
      host and the researcher has no way to reach it there.

    The copy is written on the async path only. The server runs the graph there, and the sandbox
    write is a coroutine; the sync path still fixes the arguments, so a synchronous run is
    correct, merely without the copy.
    """

    def __init__(self, sandbox_backend: Any | None = None):
        super().__init__()
        self.sandbox_backend = sandbox_backend
        #: Where the last search said it wrote. Instance state is per-request: the middleware is
        #: constructed in `_build_runtime_subagents`, which runs once per turn.
        self._server_path: str | None = None
        #: What Dataverse said matched, across every search this turn — the largest, because a
        #: broad search followed by a narrow one has still established that the broad number
        #: exists. `0` means no search reported one, which is itself worth showing.
        self._total_count = 0
        #: Whether every matching record was retrieved. Starts true and only ever goes false: one
        #: incomplete search this turn makes the accumulated file incomplete.
        self._complete = True
        #: Every record read this turn, in the order they were first seen.
        #:
        #: **Per turn, deliberately, and that is the same scope the claims check runs at.**
        #: `ClaimsRecorder` is constructed beside this one and compares at `aafter_agent`, so
        #: "everything this turn searched" is exactly the set a recommendation could have come
        #: from. Carrying it across turns would grow without bound and check a recommendation
        #: against searches nobody made today.
        self._kept: list[Any] = []

    # -- reading what a tool answered ------------------------------------------------------

    @staticmethod
    def _texts(result: Any) -> list[str]:
        """Every string a tool answer carries, whatever wrapper it arrived in.

        **The handler does not return what the tool returned.** Its contract is
        ``ToolMessage | Command`` (`langchain/agents/middleware/types.py:652`), so calling the MCP
        tool directly — which is how the first version of this was checked — exercises a shape the
        middleware never sees in production. `ToolMessage.content` is then itself either a string
        or a list of content blocks, depending on the tool.
        """
        found: list[str] = []

        def collect(node: Any) -> None:
            if isinstance(node, str):
                found.append(node)
            elif isinstance(node, dict):
                text = node.get("text")
                if isinstance(text, str):
                    found.append(text)
            elif isinstance(node, list):
                for item in node:
                    collect(item)

        # A `Command` carries its messages in `update`; a `ToolMessage` carries `content`.
        update = getattr(result, "update", None)
        if isinstance(update, dict):
            for message in update.get("messages") or []:
                collect(getattr(message, "content", message))
        collect(getattr(result, "content", result))
        return found

    @staticmethod
    def _leading_json(text: str) -> Any:
        """The JSON value a string *starts* with, ignoring anything after it.

        **`json.loads` requires the whole string to be JSON, and upstream does not send that.**
        When a result crosses `MCP_TOOL_OUTPUT_MAX_BYTES`, `_trim_json_array_text` keeps as many
        whole items as fit and appends a sentence:

            json.dumps(result_obj, indent=2) + "\n\n[60 item(s) omitted — output exceeded 124 KB…]"

        Valid JSON followed by prose. `json.loads` rejects all of it, so a search that returned a
        hundred datasets and was trimmed to forty produced **zero** — the same outcome as a search
        that failed, and for three releases it read like one (§292).

        `raw_decode` parses from the start and stops where the value ends, which is exactly the
        shape being sent.
        """
        stripped = (text or "").lstrip()
        if not stripped:
            return None
        try:
            value, _ = json.JSONDecoder().raw_decode(stripped)
        except ValueError:
            return None
        return value

    @classmethod
    def _payload(cls, result: Any) -> dict[str, Any] | None:
        """The JSON object an MCP tool answered with, whatever follows it."""
        for text in cls._texts(result):
            parsed = cls._leading_json(text)
            if isinstance(parsed, dict):
                return parsed
            # A trimmed block can arrive as a bare array — `_trim_json_array_text` rebuilds
            # `{wrap_key: kept}` only when the original had one, and drops the outer keys when
            # it did not. That is still a search result.
            if isinstance(parsed, list):
                return {"content": parsed}
        return None

    # -- setting the arguments -------------------------------------------------------------

    def _fix(self, request: Any) -> Any:
        """The call to actually run, with its path argument set rather than remembered."""
        call = getattr(request, "tool_call", None) or {}
        name = call.get("name") or ""
        args = call.get("args") or {}

        if name == SEARCH_TOOL:
            argument, wanted = "output_filename", FIXED_FILENAME
        elif name == READ_TOOL:
            argument = "file_path"
            wanted = self._server_path or f"{DEFAULT_SERVER_DIR}/{FIXED_FILENAME}"
        else:
            return request

        # `filename` is not an argument of either tool, and passing it is a hard error rather than
        # a harmless extra — it is what the previous version of this file injected.
        cleaned = {key: value for key, value in args.items() if key != "filename"}
        if cleaned.get(argument) == wanted and cleaned == args:
            return request
        # Logged when it corrects something, so the line reads "the model got this wrong again"
        # rather than "the middleware is installed".
        logger.info("%s(%s=%r) -> %r", name, argument, args.get(argument), wanted)
        return request.override(tool_call={**call, "args": {**cleaned, argument: wanted}})

    @staticmethod
    def _saved_path(result: Any) -> str | None:
        """Where the full answer was put when it was too big to hand to the model.

        The artifact first, because `response_format="content_and_artifact"` carries
        `saved_path` as a fact rather than as a sentence. The pointer text is the fallback, for a
        wrapper shape that drops the artifact on the way through.
        """
        artifact = getattr(result, "artifact", None)
        if isinstance(artifact, dict):
            saved = artifact.get("saved_path")
            if isinstance(saved, str) and saved.strip():
                return saved.strip()
        for text in SearchResultsFile._texts(result):
            found = SAVED_POINTER.search(text)
            if found:
                return found.group(1)
        return None

    @staticmethod
    def _read_text(answer: Any) -> str | None:
        """The text a backend read returned, out of whichever shape carries it.

        The same two shapes `claims.content_of` handles, written out again rather than imported:
        these two middlewares are otherwise independent, and `DATAVERSE_SEARCH` is repeated there
        for the same reason.
        """
        data = getattr(answer, "file_data", None)
        if data is None:
            return None
        content = data.get("content") if isinstance(data, dict) else getattr(data, "content", None)
        return content if isinstance(content, str) else None

    async def _payload_behind_pointer(self, result: Any) -> dict[str, Any] | None:
        """Follow the pointer to the file the answer was too big to include.

        Read back through the same backend that wrote it, so a deployment where `/workspace`
        means something different is followed rather than guessed at.
        """
        saved = self._saved_path(result)
        if not saved:
            return None
        try:
            answer = await self.sandbox_backend.aread(saved, limit=1_000_000)
        except Exception:  # noqa: BLE001 — a copy must never cost the search
            logger.exception("could not read the saved answer at %s", saved)
            return None
        if getattr(answer, "error", None):
            logger.warning("could not read the saved answer at %s: %s", saved, answer.error)
            return None
        text = self._read_text(answer)
        if not text:
            logger.warning("the saved answer at %s was empty", saved)
            return None
        return self._payload_from_saved_text(text, saved)

    @classmethod
    def _payload_from_saved_text(cls, text: str, where: str) -> dict[str, Any] | None:
        """Every record in a saved answer, however many documents it was written as.

        **A saved file is not one JSON document.** `mcp_tools._mcp_result_to_text` joins the
        tool's content blocks with `\n---\n`, so a two-block answer is two valid JSON objects
        with a delimiter between them — and `json.loads` on the whole file fails with *Extra
        data*, discarding both. That is §292's defect wearing a different separator, in the
        recovery path §291 added for it.

        Each section is parsed on its own and their records are concatenated, so a multi-block
        answer files everything rather than nothing. A section that will not parse is skipped and
        counted, because losing one block of four is a different fact from losing all four.
        """
        records: list[Any] = []
        unreadable = 0
        sections = [part for part in text.split("\n---\n") if part.strip()]
        for section in sections:
            parsed = cls._leading_json(section)
            if isinstance(parsed, dict):
                content = parsed.get("content", parsed.get("data"))
                if isinstance(content, list):
                    records.extend(content)
                    continue
                # A single record, or a shape with no list in it: keep it rather than drop it.
                records.append(parsed)
            elif isinstance(parsed, list):
                records.extend(parsed)
            else:
                unreadable += 1
        if unreadable:
            logger.warning(
                "%d of %d section(s) in %s could not be read", unreadable, len(sections), where
            )
        if not records:
            logger.warning("the saved answer at %s held no records", where)
            return None
        return {"content": records}

    # -- keeping what came back ------------------------------------------------------------

    def _remember_search(self, result: Any) -> None:
        """Note where the search wrote, and **how much it found**.

        `total_count` is the number Dataverse reported for the query, across every page — as
        opposed to `item_count`, which is how many came back. The MCP read it to decide when to
        stop paging and did not return it until today, so "found 4,000, showing 29" and "found 29"
        were the same answer at every layer (§299). A caller that cannot see the denominator
        cannot know to narrow the query, and a researcher reading twenty-nine rows cannot know
        they are a sliver.
        """
        payload = self._payload(result)
        path = (payload or {}).get("output_file")
        if isinstance(path, str) and path:
            self._server_path = path

        total = (payload or {}).get("total_count")
        kept = (payload or {}).get("item_count")
        if isinstance(total, int) and total > 0:
            self._total_count = max(self._total_count, total)
        # `complete` is the search's own verdict and a partial one must not be overwritten by a
        # later narrow query that happened to finish: once this turn has seen an incomplete
        # search, what the file holds is incomplete.
        if (payload or {}).get("complete") is False:
            self._complete = False
        if total is None:
            # Said once per search rather than never: an MCP that predates §299 answers without
            # it, and "we cannot tell you how many matched" is a different fact from "29 matched".
            logger.info(
                "%s answered without total_count — this deployment cannot say how many matched",
                SEARCH_TOOL,
            )
        else:
            logger.info(
                "%s found %s and returned %s", SEARCH_TOOL, total, kept
            )

    def _accumulate(self, content: Any) -> list[Any]:
        """Add this read's records to what the turn has already seen, in first-seen order.

        Deduplicated on the record's whole JSON, **shape-blind**, for the same reason
        `claims._ids_in` walks leaves rather than named keys: the MCP owns this layout, and a
        reader that keyed on `global_id` would silently stop deduplicating the day it was renamed.
        An unhashable or unserialisable record is kept rather than dropped — a record we cannot
        compare is not a record we may discard.
        """
        records = content if isinstance(content, list) else [content]
        seen = set()
        for kept in self._kept:
            try:
                seen.add(json.dumps(kept, sort_keys=True, ensure_ascii=False))
            except (TypeError, ValueError):
                continue
        for record in records:
            if len(self._kept) >= MAX_KEPT_RECORDS:
                logger.warning(
                    "%s is at %d records and stopped growing — later searches this turn are in "
                    "the log but not in the file, so a claims check may not find them",
                    FIXED_FILENAME,
                    MAX_KEPT_RECORDS,
                )
                break
            try:
                key = json.dumps(record, sort_keys=True, ensure_ascii=False)
            except (TypeError, ValueError):
                self._kept.append(record)
                continue
            if key in seen:
                continue
            seen.add(key)
            self._kept.append(record)
        return self._kept

    async def _keep_totals(self, work_dir: Any) -> None:
        """Record what the searches said they found, so the panel can show a denominator.

        Never raises and never costs the records: a count nobody can read is a worse outcome than
        a count nobody wrote, but only just — and losing the search over it would be far worse.
        """
        try:
            written = await self.sandbox_backend.awrite(
                f"{str(work_dir).rstrip('/')}/{SEARCH_META_DIR}/{SEARCH_META_NAME}",
                json.dumps(
                    {
                        "total_count": self._total_count,
                        "kept": len(self._kept),
                        "complete": self._complete and self._total_count > 0,
                    },
                    indent=2,
                ),
            )
            if getattr(written, "error", None):
                logger.warning("could not keep the search totals: %s", written.error)
        except Exception:  # noqa: BLE001 — a denominator is not worth a search
            logger.exception("could not keep the search totals")

    async def _keep(self, result: Any) -> None:
        """Write the metadata into the sandbox, where Outputs and the claims check can see it.

        **Accumulated across the turn, not replaced.** See [`FIXED_FILENAME`] for the false
        accusation that overwriting produced.
        """
        if self.sandbox_backend is None:
            return
        # **The whole answer first, and this is the order that matters.** `_payload` reads what
        # the *model* was given, which for a large search is a trimmed array — forty of a hundred
        # datasets, and the sentence saying so. The researcher's copy has no reason to inherit a
        # context budget, so if `mcp_tools` kept the untruncated answer aside, that is what gets
        # filed (§294).
        payload = None
        whole = mcp_tools.last_full_answer(READ_TOOL)
        if whole:
            payload = self._payload_from_saved_text(whole, "the untruncated answer")
        if not payload or "content" not in payload:
            payload = self._payload(result)
        if not payload or "content" not in payload:
            # Too big to hand to the model, so it went to a file and we were given the address.
            payload = await self._payload_behind_pointer(result)
        if not payload or "content" not in payload:
            # **Said, not swallowed.** This return was silent, and a search that found a hundred
            # datasets produced no file and nothing anywhere to say why (§291).
            capped = any(
                TRUNCATION_MARKER in text for text in self._texts(result)
            )
            # **With a sample of what did arrive.** "no JSON in the answer" was true and cost
            # another release to act on, because it does not say *what* the answer was. Two
            # hundred characters is enough to recognise a pointer, an error page, or a shape
            # nobody has met — and this is dataset metadata from a public repository, in a local
            # log the researcher already reads.
            texts = self._texts(result)
            sample = (texts[0][:200].replace("\n", " ") if texts else "nothing at all")
            logger.warning(
                "nothing to keep from %s: %s — the answer began: %s",
                READ_TOOL,
                "the answer was capped inline and the full text was not saved anywhere"
                if capped
                else "no JSON in the answer and no pointer to a saved copy",
                sample,
            )
            return
        try:
            arrived = payload["content"]
            # **Normalised before it is kept, so the file is the app's shape rather than the
            # MCP's.** The panel reads this file now; a reader in Rust that had to know Dataverse's
            # field names would break the day they changed, and the mapping belongs beside the tool
            # that produced them (§290).
            records = arrived if isinstance(arrived, list) else [arrived]
            merged = self._accumulate([normalise(record) for record in records])
            work_dir = await self.sandbox_backend.aget_work_dir()
            written = await self.sandbox_backend.awrite(
                f"{str(work_dir).rstrip('/')}/{FIXED_FILENAME}",
                json.dumps(merged, indent=2, ensure_ascii=False),
            )
            await self._keep_totals(work_dir)
            if getattr(written, "error", None):
                logger.warning("could not keep %s: %s", FIXED_FILENAME, written.error)
            else:
                # Both numbers, because "this read brought 3" and "the turn has seen 47" answer
                # different questions and the second is the one a claims warning turns on.
                logger.info(
                    "kept %s in the workspace (%d new, %d this turn)",
                    FIXED_FILENAME,
                    len(records),
                    len(merged),
                )
        except Exception:
            # A copy is a convenience. Losing it must not cost the search that produced it.
            logger.exception("could not keep %s", FIXED_FILENAME)

    # -- hooks -------------------------------------------------------------------------------

    def wrap_tool_call(
        self,
        request: Any,
        handler: Callable[[Any], Any],
    ) -> Any:
        result = handler(self._fix(request))
        if (getattr(request, "tool_call", None) or {}).get("name") == SEARCH_TOOL:
            self._remember_search(result)
        return result

    async def awrap_tool_call(
        self,
        request: Any,
        handler: Callable[[Any], Awaitable[Any]],
    ) -> Any:
        # `AgentMiddleware.wrap_tool_call` raises `NotImplementedError` with a message about this
        # exact omission, which is a good sign it is a common one.
        name = (getattr(request, "tool_call", None) or {}).get("name")
        result = await handler(self._fix(request))
        if name == SEARCH_TOOL:
            self._remember_search(result)
        elif name == READ_TOOL:
            await self._keep(result)
        return result
