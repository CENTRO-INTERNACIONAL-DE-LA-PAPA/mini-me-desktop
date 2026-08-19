"""Make the PDF librarian run something before it can report a library.

# The finding this came from

`pdf_librarian` was run for the first time on 2026-08-19 and returned a `LibraryArtifact` naming an
index and a document. §219's claims recorder checked it against the workspace:

    claims: pdf_librarian named 2 paths, 2 missing from the workspace: .asta/documents, …

`.asta/documents` is not a subtle thing to be wrong about. Measured against the real CLI:
`asta documents add` writes `.asta/documents/index.yaml` **relative to the working directory**, and
the overlay already runs every command with the conversation's workspace as that directory
(`minime_local/workspace.py`). So the index would be there if the command had run. It had not.

# Why it could happen

The exit `middleware/tool_gate.py` describes, unguarded. `response_format=LibraryArtifact` is bound
as a tool and the first model call is forced, so composing the whole artifact in one step — an index
path, a title, a page count, a summary — is the cheapest legal move available. Its prompt says:

    NEVER claim you extracted or read a paper whose PDF is not actually on disk

which is a request. `academic_researcher` and `dataverse_explorer` got gates in §133 and §142;
`pdf_librarian` was on §140's list of seven still relying on prose, and this is what that costs.

# What is forced, and why it is `execute`

All four moves the skill documents are shell commands — `fetch_paper.py` to download,
`asta pdf-extraction` to extract, `asta documents add` to index, `asta documents search` to query.
There is no library operation that is not an `execute`. So the gate is exactly one claim: **a
librarian that has run nothing has not built or searched a library**, whatever it reports.

Reading is deliberately *not* enough. `ls` and `read_file` are how a model checks whether a file
arrived, which is useful and is not the work — and a gate satisfied by looking would be satisfied by
the one thing a fabricating model would happily do.

This was written from an observed failure rather than ahead of one, which is what §219 argued for:
record first, then gate what the record shows.
"""

from __future__ import annotations

from backend.middleware.tool_gate import Step, ToolsBeforeAnswering

#: The only tool the librarian's documented moves go through.
EXECUTE_TOOL = "execute"


class RunBeforeReporting(ToolsBeforeAnswering):
    """Force one command before `LibraryArtifact` becomes reachable."""

    steps = (
        Step(
            force=EXECUTE_TOOL,
            because="pdf_librarian has not run anything, so it has no library to report",
        ),
    )
