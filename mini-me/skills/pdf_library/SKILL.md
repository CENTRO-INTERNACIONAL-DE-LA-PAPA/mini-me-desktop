---
name: pdf_library
description: >-
  Build and query a local, searchable library of PDFs and documents using the
  Asta CLI. Use to extract text from uploaded or downloaded PDFs (OCR), index
  documents into a persistent local library, and run semantic search over that
  library so downstream research and hypothesis tasks can use the user's own
  papers, institutional reports, and grey literature as grounded context.
---

# PDF Library Guidelines

Use this skill when the user wants to work with **their own documents** — papers,
institutional reports, field-trial write-ups, or any grey literature that is not
reliably indexed in public databases like Semantic Scholar.

This skill is **tool-first**. It drives the `asta` CLI inside the sandbox to:

0. **Download** open-access PDFs for referenced papers into the workspace
   (`scripts/fetch_paper.py`) — only when the files are not already on disk.
1. **Extract** text from a PDF via remote OCR (`asta pdf-extraction remote`).
2. **Index** a document into a persistent local library (`asta documents add`).
3. **Search** that library semantically (`asta documents search`).

## When to use this skill

Use when the user asks to:

- "Index this PDF / these papers" or "add this to my library"
- "Extract the text from this document" / "OCR this scanned report"
- "Search my library / my documents for X"
- "What do my uploaded papers say about Y?"

## Boundaries

This skill does **not** own:

- public literature search (use `academic_researcher` + Asta MCP)
- theory/hypothesis synthesis (use `hypothesis_generator`)
- dataset analysis (use the EDA / diagnostic / predictive subagents)

If the user wants papers *discovered* from the public literature, hand off to
`academic_researcher`. This skill manages the **local** library only.

## Auth and runtime

- `ASTA_TOKEN` is already injected into the sandbox environment, so the remote
  OCR API is authenticated. Never run `asta auth login`.
- The sandbox ships `python3` only (no `python`). Use `python3` for any scripting.
- The library index lives at `.asta/documents` under the working directory and
  persists across turns in the same thread. `asta documents ...` commands are
  local and fast; only `pdf-extraction remote` makes a network call.

## Uploaded files

When a user message starts with a blockquote like
`> Attached files (already saved in the sandbox working directory): ./report.pdf`,
those PDFs are already on disk at the given relative path (e.g. `./report.pdf`).
Do **not** ask the user to upload them again. Use those exact relative paths.

## Step 0 — Download open-access PDFs (only when the files are not on disk)

If the user asks you to read/index papers that were **discovered** (by
`academic_researcher`) but not uploaded, the PDFs are **not** in the workspace —
`asta documents add` records a URL as metadata but never fetches the bytes, and
`asta pdf-extraction` needs a local file. Use `scripts/fetch_paper.py` to pull
**open-access** PDFs into `./papers/` first. Never claim you extracted a paper
whose PDF is not actually on disk.

```python
import subprocess, json

refs = [                       # ids (preferred) or free-text titles
    "ARXIV:2005.14165",
    "DOI:10.1126/science.1255274",
    "Denoeud 2014 coffee genome convergent evolution caffeine",
]
result = subprocess.run(
    ["python3", "/skills/pdf_library/scripts/fetch_paper.py", *refs, "-o", "./papers"],
    capture_output=True, text=True,
)
records = json.loads(result.stdout)      # one record per ref
for r in records:
    print(r["ref"], "→", r["status"], r.get("path") or r.get("error"))
```

Each record has `status`:
- `downloaded` — PDF saved at `path` (under `./papers/` in the workspace). Feed
  that path into Step 1/2.
- `no_oa` — no open-access copy (likely paywalled). **Ask the user to upload the
  PDF** — do not fabricate its contents.
- `unresolved` / `fetch_failed` — reference not found, or the OA link was a
  landing page / not a PDF. Report the `error` honestly.

Scope: **open access only** (arXiv + Semantic Scholar `openAccessPdf`). This
step performs a plain network GET and depends on sandbox egress; if downloads
fail for every paper, say so and fall back to asking the user to upload.

## Step 1 — Extract text (OCR) when you need the content

`asta pdf-extraction remote` reads a local PDF and returns markdown. Use it when
you want the document's text (e.g. to build a good summary for indexing, or to
answer a question about the content).

```python
import subprocess

pdf = "./report.pdf"          # a relative path from the working dir
out = "./report.md"

result = subprocess.run(
    ["asta", "pdf-extraction", "remote", pdf, "-o", out,
     "--max-pages", "50"],     # page through large PDFs with --start-page
    capture_output=True, text=True,
)
print("rc:", result.returncode)
print(result.stderr[-2000:])
```

Notes:
- `--start-page N` (0-indexed) + `--max-pages M` page through long PDFs. For a
  120-page report, extract in chunks (`0`, `50`, `100`) into separate `.md`
  files rather than one huge call.
- `--images` also saves embedded figures next to the markdown.
- OCR can be slow for big PDFs. Keep each `execute` call bounded (one PDF or one
  page-chunk per call) so you never hit the execution timeout.

## Step 2 — Index a document into the local library

`asta documents add` records a document in the local metadata index so it becomes
searchable. It accepts a **URL or an absolute local path** (a local path is
stored as a `file://` URL). `--name` and `--summary` are **required** — the
summary is what semantic search matches against, so make it substantive.

```python
import subprocess, os

pdf_abs = os.path.abspath("./report.pdf")
name = "CIP 2023 Late Blight Field Trial — Cañete"
# Build the summary from the extracted markdown (Step 1): 3-6 sentences covering
# the topic, methods, crops/varieties, location, and key findings. A rich
# summary makes semantic search far more useful than a one-liner.
summary = "Field trial evaluating late blight resistance across 12 potato ..."
tags = "potato,late-blight,field-trial,peru"

result = subprocess.run(
    ["asta", "documents", "add", pdf_abs,
     "--name", name, "--summary", summary, "--tags", tags,
     "--mime-type", "application/pdf", "--json"],
    capture_output=True, text=True,
)
print(result.stdout)          # JSON record incl. the generated uuid
```

- One `add` call per document. For a batch, loop over the files.
- Always write a real, content-derived `--summary`; the search quality depends on
  it. If you only have the title, extract at least the first pages first (Step 1).
- Tags are comma-separated and improve `--tags` search.

## Step 3 — Search the library

`asta documents search` does semantic search over the indexed name / summary /
tags. Prefer `--json` and `--show-scores` so you can inspect matches.

```python
import subprocess, json

result = subprocess.run(
    ["asta", "documents", "search", "--summary", "late blight resistance",
     "--limit", "10", "--show-scores", "--json"],
    capture_output=True, text=True,
)
print(result.stdout)
```

- `--summary Q` searches summaries (the main content signal); `--name Q` and
  `--tags Q` search those fields. Combining fields intersects by default; add
  `--union` to OR them.
- Use `asta documents list --json` to enumerate everything, and
  `asta documents show` for index-level counts.

## Mapping to `LibraryArtifact`

Return a `LibraryArtifact` structured response describing what you did this turn:

- `action` — `"index"`, `"extract"`, or `"search"`.
- `summary` — one or two sentences on the outcome (e.g. "Indexed 3 PDFs" or
  "Found 4 documents matching 'late blight resistance'").
- `paper_count` — total documents now in the library. Get it from
  `asta documents list --json` (count the records) or `asta documents show`.
- `index_path` — leave as the default `.asta/documents` unless you used `--root`.
- `papers` — the documents relevant to this turn: the ones you just indexed, or
  the search matches. For each: `title` (the `name`), `path` (the `url`/path),
  `doi` if known, `summary`, `tags`, and `page_count` if known.
- `query_hint` — tell the user how to query next, e.g. "Ask me to search your
  library for a topic, or hand these papers to the researcher for synthesis."

Do not fabricate documents or matches. If OCR or indexing fails, report the
stderr honestly and return an empty `papers` list with an explanatory `summary`.
If a search returns nothing, say so and suggest indexing more documents.
