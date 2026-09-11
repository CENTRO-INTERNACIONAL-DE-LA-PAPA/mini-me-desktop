# Mini-Me Desktop (Ask the Data)

<p align="center">
  <img src="mini-me/images/mini_me.png" alt="Mini-Me" width="420" />
</p>

**Mini-Me is an AI research workbench that coordinates specialist agents to find evidence and datasets, analyze your files, test hypotheses, and produce traceable reports.**

Mini-Me and **Ask the Data** are the same system. `AsktheData-Agent` is the name of the
coordinator graph in the backend; Mini-Me is the product name used by the desktop application.

Mini-Me Desktop is the native Windows client for the
[Mini-Me](https://github.com/CENTRO-INTERNACIONAL-DE-LA-PAPA/Mini-Me) research-agent
stack. The interface is written in Rust with [GPUI](https://crates.io/crates/gpui), while
the Python/LangGraph backend runs as a local sidecar. The application is designed for
researchers who want one workspace for literature review, data discovery, data analysis,
scientific reasoning, and reporting without having to operate the individual tools by hand.

> **Current status:** active development, workspace version **0.3.30**. Windows is the primary
> platform. The current mainline backend runs through WSL2; the separate `feat/wsl2less`
> development branch has deliberately not been merged.

## What Mini-Me does

A coordinator receives the researcher's request, delegates bounded pieces of work to the
appropriate specialists, and combines their results in one persistent conversation. A request
can move through the complete research workflow:

1. Define a mission and prepare a research plan.
2. Search peer-reviewed literature and the researcher's own PDF collection.
3. Find and inspect datasets in CIP Dataverse.
4. Validate, clean, and standardize tabular data.
5. Explore patterns, missingness, distributions, outliers, and correlations.
6. Investigate associations, mechanisms, confounding, and causal questions.
7. Train and evaluate predictive, statistical, time-series, or Bayesian models.
8. Generate and test literature-grounded hypotheses.
9. Produce a referenced Markdown report and a styled PDF.

The researcher can let the coordinator choose the team or invoke a specialist directly. The UI
streams each specialist's activity, keeps its outputs beside the conversation, and records where
the resulting evidence and files came from.

## The specialist team

The live backend currently defines twelve specialists. The desktop reads their names and
descriptions from the backend registry instead of maintaining a separate hardcoded UI list.

| Specialist | Responsibility |
|---|---|
| `research_planner` | Writes a concise, ordered investigation plan for the researcher to review. Planning does not execute the work. |
| `academic_researcher` | Searches scientific literature with Asta, synthesizes evidence, and returns citations and stable links. |
| `dataverse_explorer` | Searches CIP Dataverse, reads complete result metadata, and recommends datasets with persistent identifiers. |
| `pdf_librarian` | Extracts and indexes the researcher's PDFs into a conversation-specific semantic library and searches it by meaning. |
| `data_cleaning` | Validates schemas, identifies data-quality problems, harmonizes terminology, and writes cleaned versions without replacing raw files. |
| `exploratory_data_analysis` | Profiles and summarizes data, examines missingness and outliers, and creates explanatory visualizations. |
| `diagnostic_analytics` | Investigates why an outcome occurred using comparisons, regression, inference, confounder checks, and causal framing. |
| `predictive_analytics` | Selects, trains, validates, and compares machine-learning, forecasting, deep-learning, and Bayesian models. |
| `data_voyager` | Uses Asta DataVoyager to test a specific analytical question against local tabular data and return findings and charts. |
| `hypothesis_generator` | Uses the Asta Theorizer pipeline to generate literature-grounded mechanisms, theories, and open questions. |
| `autodiscovery` | Drafts an open-ended Asta exploration that generates and tests hypotheses over a dataset. The researcher approves its experiment budget before it runs. |
| `report_writer` | Synthesizes findings, methods, limitations, recommendations, and references into a complete report. |

## Desktop experience

### Conversations and projects

- Persistent, multi-turn conversations backed by LangGraph threads.
- Follow-up questions retain the context of the conversation.
- Searchable conversation history.
- Projects for grouping related conversations and their files.
- A project mission plus completed, active, and proposed work in the Road panel.
- Creation, filing, moving, opening, and deletion of conversations and projects.
- A `Ctrl-P` command palette for common actions.

### Chat and agent activity

- Streaming answers with Markdown, tables, lists, links, code blocks, and images.
- Local file attachments copied into the conversation before the agent receives them.
- Expandable activity traces that show which specialist did what.
- Text selection and clipboard commands across the transcript.
- Optional background specialists for work that should continue while the chat remains usable.
- Visible states for running, waiting for approval, completed, failed, and input-required work.

### Research outputs

The Outputs panel presents structured artifacts separately from conversational prose:

- Files and figures, including previews and image lightboxes.
- Literature sources and stable citation links.
- Dataverse results with identifiers, metadata, access information, and download actions.
- Reports that can be read as Markdown or rendered as PDF.
- Complete per-conversation PDF libraries.
- Hypotheses, theories, analyses, and experiment results.
- Long-running Asta jobs and background tasks.
- A provenance view showing which specialists and inputs produced an output.

Diagnostic summaries of commands and recorded claims are available but hidden by default. They
can be enabled under **Settings → Backend → Show what ran and what was claimed**.

## Files and conversation workspaces

The researcher's work belongs in Documents, not in an application cache. Every conversation has
its own folder beneath:

```text
C:\Users\<user>\Documents\Mini-Me\
```

An ungrouped conversation is stored as `<thread-id>`. A conversation filed into a project is
stored inside that project's folder. The folder can contain:

- Uploaded copies of input data and PDFs.
- Cleaned datasets and validation results.
- Charts, figures, notebooks, and analysis files.
- Markdown and PDF reports.
- Literature and Dataverse search records.
- PDF-library indexes and extracted text.
- Provenance, command, and claim records.
- Results collected from background workers.

The desktop passes the conversation folder as the working directory for the agent. It also
observes command output and can offer to bring files back when a tool writes into another known
working directory. Existing user files are never silently overwritten during adoption.

### PDF libraries

The PDF Librarian keeps a separate collection for each conversation:

```text
<conversation>\.asta\documents\index.yaml
<conversation>\.asta\documents\.cache\search.db
```

`index.yaml` is the durable inventory and metadata source. `search.db` contains the full-text and
semantic-search cache. The Library modal reads the inventory rather than treating the most recent
search matches as the whole collection. Existing local PDF rows can be clicked to open the source
document, including files whose Asta location is recorded as `file:///mnt/c/...`.

### For whoever prepares the build

Nothing to run first — the backend is `mini-me/`, tracked directly in this repository (no
separate private repo, no personal access token, no bundling step). Build the app and package
it:

```bash
cargo build --release -p mini-me-desktop-app
bash scripts/package.sh
```

`package.sh` copies `mini-me/` into the bundle, so every install after that provisions from it
without ever contacting GitHub.

## MCP integrations

Ask the Data loads four hosted Model Context Protocol servers over HTTP. LangChain's first-party
MCP adapter and FastMCP 4 negotiate the stateless 2026 protocol with a legacy fallback per server;
tool discovery follows each server's cache TTL. An unavailable deployment removes only its own
capability, and oversized results are saved into the active conversation workspace rather than
filling the model context. Modern MCP form and URL elicitation pauses are shown to the researcher
and may be accepted, declined, or cancelled before the tool continues.

| MCP server | Endpoint | Used by | Purpose and exposed scope |
|---|---|---|---|
| **Asta** | `https://asta-tools.allen.ai/mcp/v1` | `academic_researcher` | Scientific literature search and passage retrieval. Authenticated with an Asta API key. |
| **CIP Dataverse** | `https://dataverse-cip.fastmcp.app/mcp` | `dataverse_explorer` | Dataset search, complete search-result reading, and dataset-file listing. The agent is restricted to `SearchCIPDataverse`, `read_search_results`, and `list_dataset_files`; it does not receive curation tools. |
| **AGROVOC** | `https://agrovoc.fastmcp.app/mcp` | `data_cleaning` | Agricultural vocabulary lookup and terminology normalization. |
| **Crop Ontology** | `https://CropOntology.fastmcp.app/mcp` | `data_cleaning` | Crop trait, genotype, and phenotype terminology and mappings. |

The Asta-powered specialists also use the authenticated `asta` CLI for capabilities that are not
exposed through those MCP tools:

- `asta papers` for structured Semantic Scholar paper records.
- `asta pdf-extraction remote` for PDF text and OCR.
- `asta documents` for the local semantic PDF library.
- `asta generate-theories` for the Theorizer pipeline.
- `asta analyze-data` for DataVoyager.
- `asta autodiscovery` for credit-metered open-ended experiments.

## External services

| Service | Why it is used | When data leaves the machine |
|---|---|---|
| **LLM providers** | Reasoning and language generation. OpenAI, Anthropic, Google, Mistral, and custom OpenAI-compatible gateways are supported. | When a conversation or specialist run is submitted. The user supplies the provider credentials. |
| **Asta / Allen Institute for AI** | Literature tools, Semantic Scholar records, PDF OCR, document embeddings, theory generation, DataVoyager, and AutoDiscovery. | Only when an Asta-backed specialist or command is used. Some operations consume Asta credits and require approval. |
| **CIP Dataverse MCP** | Searches CIP's dataset catalogue and retrieves dataset/file metadata. | When the Dataverse Explorer runs. |
| **AGROVOC and Crop Ontology MCPs** | Resolve and normalize agricultural and crop terminology. | When the Data Cleaner requests ontology assistance. |
| **Semantic Scholar and arXiv** | Stable paper records and open-access PDF locations. These are normally reached through Asta; the PDF Librarian can download an identified open-access PDF URL. | When literature is resolved or an open-access paper is explicitly fetched. Paywalled material is not bypassed. |
| **Crossref** | Checks whether a DOI resolves to the paper named by a citation and can search for a likely registered DOI. | The desktop sends a DOI, or citation text when repairing a missing/mismatched identifier. It does not send the research question or uploaded files. |
| **LangGraph / LangSmith** | Agent graph execution, threads, state, streaming, and the optional remote sandbox/production deployment. | Local desktop development uses a local LangGraph server. Data reaches LangSmith when remote sandbox, hosted deployment, or tracing is enabled. |
| **WorkOS** | Authentication and encrypted per-user credential storage for the hosted web deployment. | Used in production web/Vault mode. The desktop instead stores model keys in the OS keychain. |
| **Amazon S3 and CloudFront** | Optional deployment target for the React web client. | Only for the separately deployed web frontend; not required by the desktop client. |
| **GitHub Releases** | Publishes and checks digest-verified desktop release bundles. | The desktop checks the repository's latest-release endpoint; research data is not sent. |

Researcher-provided PDFs and tabular files are another primary data source, but they are inputs
rather than an external service. In local mode they remain in the conversation workspace except
for the selected model request and any explicitly invoked external analysis or search service.

Literature search is powered by Asta from the Allen Institute for AI. Work that uses Asta output
should cite:

> *AstaBench: Rigorous Benchmarking of AI Agents with a Scientific Research Suite.*
> arXiv:2510.21652.

## Safety, privacy, and spending

Mini-Me is designed around explicit researcher control:

- Model API keys are stored in the operating-system keychain, not in the repository, `.env`, or
  application logs.
- Keys travel with the individual run request and are not installed into the backend environment.
- Local Python and shell commands can pause and display the exact command before execution.
- Approval can cover one command, the rest of the current turn, or the current conversation.
  Approval grants are temporary and are not persisted.
- Asta operations that spend credits require a separate human approval. AutoDiscovery exposes the
  proposed experiment count and cost before submission.
- Local execution is the desktop default; a remote LangSmith sandbox remains available when
  explicitly selected.
- Files and structured claims are recorded for traceability, but Mini-Me cannot prove every claim
  written in free-form model prose.

Generative AI produces analysis and prose in this application. Researchers must validate results
with appropriate subject-matter experts, avoid entering confidential or restricted information,
disclose AI use where required, and review citations before publication.

## Architecture

```text
Native GPUI desktop
        │
        ├── conversations, projects, settings, approvals, outputs
        │
        ▼
Rust sidecar supervisor ── LangGraph HTTP/SSE ── Python coordinator
                                                    │
                                                    ├── specialist agents
                                                    ├── Asta and MCP tools
                                                    ├── local/remote execution
                                                    └── structured artifacts
        │
        ▼
Per-conversation folders under Documents\Mini-Me
```

The desktop application:

- Starts the backend or attaches to a healthy instance already listening on the configured port.
- Provisions an owned backend inside WSL when necessary.
- Streams coordinator and subagent events over Server-Sent Events.
- Converts backend artifacts into native GPUI panels and modals.
- Injects the desktop-only Python package in [`overlay/`](overlay/) through `PYTHONPATH`.
- Keeps model credentials in the OS keychain and sends configuration with each run.

The overlay provides local workspaces, command approval, artifact and provenance recording,
background-agent support, and desktop-specific behavior without requiring those adaptations to be
maintained as edits to an external Mini-Me checkout.

### Integration surface

The LangGraph graph is registered as assistant `agent` and implemented by
`backend/agent.py:agent`; the constructed coordinator is named `AsktheData-Agent`. An integrating
client supplies a LangGraph thread id, user messages, optional attachments, project context, model
routing, and credentials. Runs stream messages, tool activity, interrupts, and whole state
snapshots over LangGraph's HTTP/SSE protocol.

Specialists return typed artifacts rather than requiring another system to scrape prose:

- `AcademicResearchResults` for literature and citations.
- `DataVerseSearchResults` for dataset recommendations.
- `LibraryArtifact` for PDF extraction, indexing, and semantic search.
- `HypothesisOutput` for theories and evidence.
- `DataAnalysisResults` for DataVoyager findings and figures.
- `DiscoveryRunResults` for AutoDiscovery lifecycle and experiments.
- `ReportWriterOutput` for complete report content.
- `ResearchPlan` for ordered, reviewable investigation steps.

Custom backend routes cover file upload/download, PDF report rendering, project management,
provider configuration, Asta authentication, sandbox lifecycle, stray-output collection, and
status/approval flows for Theorizer, DataVoyager, and AutoDiscovery. This makes it possible for an
AI-CoScientist interface to reuse the agent backend without reproducing the desktop UI, provided
it preserves thread identity, structured artifacts, interrupts, credential isolation, and the
human credit-approval gate.

## Technology stack

| Layer | Languages and principal components |
|---|---|
| **Desktop client** | Rust 2021, GPUI 0.2.2, Tokio, Reqwest, Serde, and the native Windows credential manager through `keyring`. |
| **Agent backend** | Python 3.12+, LangGraph API/SDK, LangChain, Deep Agents, Starlette, and Pydantic structured responses. |
| **Optional web client** | TypeScript 5.9, React 19, Vite 7, LangChain React, and WorkOS AuthKit. |
| **Data science** | pandas, NumPy, SciPy, scikit-learn, statsmodels, PyMC, PreliZ, DABEST, XGBoost, UMAP, pointblank, missingno, Matplotlib, and Seaborn. |
| **Documents and reports** | Markdown, `pypandoc-binary`, Typst, PDF extraction/OCR, and BibTeX/reference rendering. |
| **Protocols** | HTTP/JSON, Server-Sent Events for live agent output, MCP over HTTP for external tools, and local CLI/subprocess execution. |
| **Persistence formats** | LangGraph thread/checkpoint state, JSON artifacts, YAML indexes, Markdown reports, SQLite search/index caches, and ordinary files in the conversation workspace. |
| **Build and operations** | Cargo/Rust tooling, `uv` for Python environments, npm for the optional web client, Bash/Git Bash, PowerShell, GitHub Actions, and Windows SDK shader compilation. |

## Settings

`Ctrl-,` opens Settings. Available configuration includes:

- Provider and coordinator model.
- Per-specialist model overrides.
- Provider API keys.
- Custom OpenAI-compatible base URLs, including gateways such as OpenRouter.
- Local-machine versus remote-sandbox execution.
- Per-command approval.
- Background specialist execution.
- Optional command/claim diagnostics.
- Application theme.

Supported provider families are Anthropic, OpenAI, Google, Mistral, and custom
OpenAI-compatible endpoints. Keys are managed separately per provider so selecting a model also
makes its billing authority visible.

## Installing and running on Windows

Windows is the supported user platform. The released application guides the user through missing
requirements in its **Setup & diagnostics** page. The current mainline requires:

- Windows 10 or 11.
- WSL2 with a Linux distribution.
- A configured model-provider API key.
- Asta sign-in for Asta-backed capabilities.

The Setup page can provision the Mini-Me backend and Python dependencies. Installing WSL itself
requires administrator rights and can require a restart.

The backend is provisioned inside WSL at:

```text
~/.local/share/mini-me-desktop/backend
```

This is separate from the researcher's conversation folders. Updating the desktop executable does
not automatically replace an already provisioned backend; the Setup page reports a content-stamp
mismatch and provides the backend update action.

## Development

### Requirements

- Rust stable, selected by [`rust-toolchain.toml`](rust-toolchain.toml).
- WSL2 for the current Windows backend path.
- A Mini-Me backend checkout or the copy under [`mini-me/`](mini-me/).
- Windows SDK with `fxc.exe` for optimized GPUI builds.
- Git Bash for the packaging scripts.

### Run a development build

From PowerShell:

```powershell
$env:MINIME_BACKEND_WSL=1
cargo run -p mini-me-desktop-app
```

To use a specific Mini-Me checkout inside WSL, set `MINIME_BACKEND_WSL_DIR`. To use a native
checkout where supported, set `MINIME_BACKEND_DIR`. A checkout supplied by the developer is treated
as borrowed: the desktop runs it but does not provision or rewrite it.

### Run an optimized build

## Backend prerequisite

Provisioned for you by the Setup pane. By hand, it is:

```bash
bash scripts/setup-wsl.sh [target-dir]
```

**`--extra dev` matters** (the script passes it) — the LangGraph CLI is an optional
extra, so plain `uv sync` leaves you with no `langgraph` entry point. Keys do **not**
go in that checkout's `.env` any more: they live in your OS keychain and travel with
each request, so the app needs no secrets on disk.

## Git inside WSL asks for a password

Only on the **Windows** side does git have a credential helper; inside the distro it does
not, so `git pull` there prompts — and GitHub has not accepted account passwords since
2021, so the prompt cannot be satisfied. Reuse Windows' credential manager:

```bash
git config --global credential.helper "/mnt/c/Program Files/Git/mingw64/libexec/git-core/git-credential-manager.exe"
```

(If that path is wrong, `ls /mnt/c/Program\ Files/Git/mingw64/libexec/git-core/ | grep credential`.)

## Release builds need `fxc.exe` (Windows)

A **release** build of `gpui 0.2.2` pre-compiles its HLSL shaders; a debug build does not
(`build.rs:259` gates the step on `#[cfg(not(debug_assertions))]`). So `cargo build` can
work for months and `cargo build --release` still fail with:

```
Failed to find fxc.exe
```

`fxc.exe` is the DirectX shader compiler from the **Windows SDK**. gpui looks for it in
`GPUI_FXC_PATH`, then on `PATH`, then at one hardcoded SDK version
(`10.0.26100.0`) — so having a *different* SDK version installed is enough to fail.

Point it at yours (PowerShell):

```powershell
$env:GPUI_FXC_PATH = (Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -Recurse -Filter fxc.exe -ErrorAction SilentlyContinue | Sort-Object { $_.FullName -notmatch '\\x64\\' }, FullName -Descending | Select-Object -First 1).FullName
```

Confirm that the command found a file, then run:

```powershell
cargo run --release -p mini-me-desktop-app
```

If the search returns nothing, install the Windows 11 SDK from Visual Studio Installer.

### Test

```powershell
cargo check --workspace
cargo test --workspace
```

The repository also contains Python tests for the vendored backend. Some integration tests require
the Python/Asta environment and external credentials; unit tests must not spend model or Asta
credits.

### Headless diagnostics

Run the same checks shown by the Setup page without opening a window:

```powershell
cargo run -p mini-me-desktop-app -- --preflight
```

Exercise backend startup and health checks:

```powershell
cargo run -p mini-me-desktop-app -- --check-backend
```

Exercise a complete streamed turn, which can spend model tokens:

```powershell
cargo run -p mini-me-desktop-app -- --check-backend --stream
```

Or provide an explicit prompt:

```powershell
cargo run -p mini-me-desktop-app -- --check-backend --prompt "find recent late-blight datasets"
```

Decode a recorded SSE stream without starting the backend or spending tokens:

```powershell
cargo run -p mini-me-desktop-app -- --replay crates/app/tests/fixtures/delegated-turn.sse
```

Store a credential in the OS keychain without displaying it in the UI:

```powershell
cargo run -p mini-me-desktop-app -- --set-secret llm:anthropic "sk-..."
```

Useful overrides include `MINIME_BACKEND_WSL`, `MINIME_BACKEND_WSL_DIR`,
`MINIME_BACKEND_DIR`, `MINIME_BACKEND_PORT`, `MINIME_BACKEND_URL`,
`MINIME_BACKEND_ATTACH_ONLY`, and `MINIME_EXECUTION_BACKEND`. The `--local` and `--sandbox`
flags override execution locality for one launch.

### Host execution (the only mode)

The agent's code runs **on this machine** — no LangSmith key, no cold start, no upload
dance. Files land in `~/.mini-me/workspaces/<thread>/`, where you can open them yourself.
There is no remote-sandbox alternative to opt into; local execution is unconditional.

**Every `execute` call stops and asks first.** The run pauses, the app shows you the
command verbatim, and nothing runs until you approve it. That is what makes running on
your own machine reasonable rather than reckless. `MINIME_APPROVE_EXECUTE=0` disables the
gate — it exists for automation, and is not a recommendation.

This is implemented directly in `mini-me/backend/local/` — part of the backend package
itself, not an external checkout patched at import time (see plan §18/§19 for the
history: this used to be a `PYTHONPATH` overlay kept separate so it never conflicted with
an upstream Mini-Me checkout; now that the backend is vendored in this repo, that
separation no longer serves a purpose).

### Logs

On Windows, the three primary logs are:

```text
%TEMP%\mini-me-desktop-app.log
%TEMP%\mini-me-desktop-backend.log
%TEMP%\mini-me-desktop-update.log
```

Include the build stamp shown in **About Mini-Me** and the relevant logs when reporting a defect.

## Packaging and release

Before packaging on a machine that can access the Mini-Me repository, prepare the backend bundle:

```bash
bash scripts/bundle-backend.sh
```

The manual Windows release sequence is run from Git Bash:

```bash
cargo build --release -p mini-me-desktop-app
cargo test --release
bash scripts/bundle-backend.sh
bash scripts/package.sh
bash scripts/release.sh
```

The GitHub Actions [`release` workflow](.github/workflows/release.yml) performs the corresponding
Windows build, tests, packaging, and draft-release creation. Releases are drafted first. Before
publishing, inspect the produced archive and confirm that `overlay/`, `scripts/`, `vendor/`, and
`mini-me/` contain the files expected by the installed application.

## Repository layout

```text
crates/app/       Native Rust/GPUI application and sidecar supervisor
mini-me/          Mini-Me Python/LangGraph backend snapshot and specialist skills
overlay/          Desktop-only Python behavior injected into the backend
scripts/          Setup, backend bundling, packaging, release, and rehearsal tools
docs/             Handover, working plan, design record, and upstream issue notes
dist/             Locally produced release artifacts
vendor/           Bundled-backend compatibility location used by packaging
```

## Documentation

Start here when maintaining the application:

1. [`docs/handover.md`](docs/handover.md) — safety rules, architecture, failure history, and release
   guidance. Its release/PR snapshot can lag behind the repository.
2. [`docs/plan.md`](docs/plan.md) — the current work checklist and intentionally deferred work.
3. [`docs/desktop-app-plan.md`](docs/desktop-app-plan.md) — the long-form source of truth. Each
   numbered section records a problem, evidence, decision, and implementation; `§N` code comments
   refer back to it.
4. [`overlay/README.md`](overlay/README.md) — how desktop-local execution is injected.
5. [`mini-me/README.md`](mini-me/README.md) and [`mini-me/skills/`](mini-me/skills/) — the backend
   architecture and detailed specialist procedures. Some summary counts in the backend README
   predate the newer specialists; the live registry and `backend/subagents.py` are authoritative.

When behavior contradicts the source tree, inspect the installed or released artifact and the
provisioned backend before assuming the running application contains the latest code. The desktop
binary, backend installation, and copied overlay are three distinct pieces that can be at different
revisions.

## Acknowledgements

Mini-Me Desktop is developed for the International Potato Center (CIP). It uses GPUI from Zed and
Asta from the Allen Institute for AI. See [`NOTICE`](NOTICE) for third-party attribution.
