# Mini-Me Desktop — Phase 6 plan & status

A native **desktop research-acceleration workbench** for Mini-Me, built in Rust
on **GPUI** (the GPU UI framework extracted from [Zed](https://github.com/zed-industries/zed)).
This repo is the desktop **client**; the Mini-Me agent stack (the coordinator +
Asta-backed subagents + skills) stays in Python/TypeScript and runs as a **local
sidecar** that the client spawns and supervises.

## Where we are now (updated 2026-08-02)

| Milestone | Status |
|---|---|
| **P6.0** — spike doc + scaffold | ✅ done |
| **P6.1** — buildable window *(go/no-go gate)* | ✅ **PASS** — builds green; window renders natively (verified on Windows/DirectX). §8 |
| **P6.2** — talk to the real backend | ✅ **done** — a real coordinator turn spawned, streamed and rendered **on Windows** (2026-07-30). §9 |
| **P6.2.5** — local-first backend (drop LangSmith/WorkOS) | ✅ **done, and now the default** — turns run on the host with no `LANGSMITH_API_KEY`/`WORKOS_*`, via a `PYTHONPATH` overlay that leaves the Mini-Me checkout untouched, and **every `execute` call waits for approval**. `--sandbox` still available. §18/§19 |
| **P6.3** — port the core panels | ✅ **done** — composer, spine, outputs, sandbox status, agent activity trace, **command palette**; plus conversation continuity (turns used to each start a new thread) |
| **P6.3.5** — visuals pass, starting with **markdown rendering** | ✅ **verified on Windows** — emphasis, inline code, links, headings, lists and fenced code render; accented Spanish came through intact. Tables deferred by agreement. §16/§23 |
| **Native-Windows probe** | ✅ **answered** — `cmd.exe` is ruled out by upstream's *own* tool code (POSIX pipes, `mkdir -p`, `shlex.quote`), so WSL2 stays the v1 runtime and the installer's job is guided provisioning. Native-plus-Git-Bash is a documented half-day experiment. §21 |
| **P6.4a** — settings panel + keychain secrets | ✅ **built** — a turn runs with no provider key in the backend's `.env`; keys come from the OS keychain and ride in the run request, and `ctrl-,` opens a Settings pane (provider, model, keys, execution). **Never rendered — needs a look on Windows.** §20/§22/§22b |
| **P6.4b** — native affordances + shipping | ✅ **ships** — `bundle-backend.sh` → `--release` → `package.sh` gives a 21 MB folder; **the packaged build ran a real turn on Windows**. **Job Object verified 2026-08-01** — after closing the app, `wsl -- pgrep -af "langgraph dev"` prints nothing. Resources resolve beside the executable, and **a file dropped on the window — or added with the composer's clip — becomes part of the question**, with a path the agent can open and a refusal for one it cannot. Remaining: click-to-update, and **actually dragging a file onto a real window**, which nobody has done. §24/§25/§26/§28/§179 |
| **P6.5** — background work + Jobs panel | ✅ **done, end to end (2026-08-01)** — background work had in fact never run until §39: our graph factory took no `config` and raised `TypeError` at construction. Now a worker generates data, **stops at the approval gate on its own thread**, and the answer reaches it. Failures report the real exception, and the panel shows which subagent is running. §29–§31/§36–§42 |
| **P6.6** — outputs the researcher can see | ✅ **done** — files land in `Documents\Mini-Me\<thread>`, figures render in the chat, and OUTPUTS opens the folder. §42 |
| **P6.7** — the UI itself | ✅ **done, verified on Windows** — a role-based palette with four built-ins **and Zed's whole theme gallery** installable in-app; conversation sidebar with fuzzy search and rename; collapsible panels; a file preview modal; visible scrollbars; rainbow CSV; a three-state send button; rounded panels and a window-wide status bar. §43/§47–§53 |

### What is left (updated 2026-08-09)

Every milestone P6.0–P6.7 is closed and the app is in daily use by its first researcher.
What remains splits three ways: **shipping it to anyone else**, **paying down the UI debt
that keeps causing the same bug**, and **friction that is felt but not blocking**.

**Blocks a second person using it**
- ✅ **First run where WSL has never existed.** Proven on a third laptop (§61): elevated
  install, restart, reopen — **4 ok**, with the runtime, checkout, `uv sync` and overlay all
  green without anyone typing a command. Took three rounds: §57 fixed elevation, §60 made the
  elevated output readable and stopped the app claiming "done" over a red row, §61 reverted a
  `--no-launch` flag that could have unregistered the distro.
- 🟡 **The download link.** `v0.1.0` is tagged, built green, and attached as
  `mini-me-desktop-v0.1.0-windows-x64.zip`. The release is a **draft** — deliberately, since
  a draft can be deleted where a published release cannot. **Remaining: someone decides to
  publish it**, which is a call about who is allowed to have this, not an engineering step.
- ⬜ **Code signing.** SmartScreen shows "Windows protected your PC" and most researchers
  stop there. An organizational decision on a certificate; the release notes and README
  say which two words to click in the meantime.
- ✅ **Click-to-update** — answered by making it unnecessary (§135/§139). The backend source
  ships in this repository at `mini-me/`, and the launch mirrors it into the checkout, so
  **`git pull` on the app *is* the backend update**. The `git fetch` it replaces was against a
  private remote WSL has no credentials for: it hung on a sign-in nobody was watching (§131), or
  failed fast and left the checkout a month behind while every log line read healthy (§134).

**UI debt — the same call-site mistake, six times now**
- ✅ **`Button`, `Label`, `Modal`, `Toggle`** (§67, §68) — confirmed on a real window across all
  five pages. 19 button sites, 6 label sites; **twelve square buttons rounded** on the way,
  including `Install Ubuntu` and `Copy ⧉`. `rounded_md` and `flex_none` are no longer reachable
  to omit, `disabled` is one flag rather than a colour beside a guard, `Label::ellipsis()`
  cannot produce §59's collapse, and `body`/`actions` being separate slots leaves "actions
  inside the scroll area" nowhere to happen. Setup is a page of the preferences window instead
  of a column that evicted the research panel. Two controls stay hand-written, each saying why.
- ✅ **The palette runs the row it highlights** (§69) — the selection was computed three ways and
  only the Enter path did not clamp, so past the end of a filtered list it silently did nothing.
  One function now, and the empty case says so out loud.
- 🟡 **UI polish** — all built (§70), **awaiting eyes**: theme and model as searchable dropdown
  popups; fenced code in a monospace stack (nothing bundled — `Consolas` ships with Windows);
  focus rings plus a tab order through each page's fields; toasts that stack and fade instead of
  one overwriting status line; panes resizable by dragging their edge.
- ✅ **Transcript re-parse** (§70) — markdown was being parsed *in `render`*, so every message
  was re-parsed sixty times a second. Cached beside the body now. The list's own entry said
  `uniform_list`, which was **wrong**: it lays every element out at the height of the first, and
  these are a one-line question next to a two-page report.
- ✅ **Virtualize the transcript** with `list` + `ListState` (§156) — the honest version of the above,
  wanted only once a conversation is long enough to feel it.
- ✅ **SVG icons** instead of `◎ ▤ ▥ ⏎` (§157, §171) — was deferred (§70) because it needs an `AssetSource` and
  hand-authored assets, to replace glyphs that render correctly, for no functional gain.

**Felt friction**
- ✅ **Multi-line composer** — Shift-Enter inserts a break, Enter still sends (§55).
- ✅ **Escape closes things**, inside-out; **conversations and projects can be deleted** from a
  centred warning that names the chat history and saved folders that will go (§58/§155);
  **theme and model are filterable lists**; **corners are rounded** (§58/§59).
- ✅ **Text selection in the transcript** (§62) — drag across paragraphs, code blocks and table
  cells; `ctrl-c` copies, `ctrl-shift-a` takes everything, both also in the palette.
  Confirmed on a real window.
- ✅ **Cancel a running turn** (§63) — confirmed on a real turn stopped three steps in: the
  partial trace stayed, marked incomplete. Stop posts to LangGraph's cancel endpoint *and*
  aborts the stream, so the graph stops spending tokens rather than just losing its audience.
  The cancel now logs on success as well as failure, since whether the *backend* stopped is
  the one part nobody can see.
- ✅ **Right-click menu** (§64) — confirmed: copy/select-all in the transcript, cut/copy/paste
  in the composer, rows greyed when they would do nothing, each showing its own binding.
- ✅ **Cancel a running setup fix** for ordinary repairs (§28 → §146 → §168 → §170 → §172).
  Still open: **elevated installs**, which sit outside this app's token and need §168's second-UAC
  policy and a disposable-VM test before Stop can honestly appear on them. §168 now specifies exactly what a
  truthful Stop requires: a published numeric PGID and `kill -- -PGID` for the ordinary WSL
  repair, a **second** UAC-approved `taskkill /T` for an elevated install, and five tests —
  four of which need the target Windows/WSL pair and one a disposable VM. Still deliberately
  unbuilt: a Stop button at the layer that owns only an event receiver would change the
  screen and nothing else, which is the dishonest outcome §146 refused.
- ✅ **Markdown gaps** (§65, §66) — verified on a real answer: blockquotes, nested lists whose
  depth comes from the indents actually seen (two- and four-space sources rendered identically,
  which was the point), images no longer showing their own punctuation. The one defect the
  eleven unit tests missed — a `>>` after a `>` folding into the outer quote — was found by
  that answer in seconds, because every test covered a construct alone and the bug lived in the
  transition between two.

**Background work — found on the machine, 2026-08-10**
- ✅ **`GET /threads/{id}/state` returned 500 on every poll** (§148) — `build_chat_model` claimed
  in its docstring that constructing without a key never raises, which is false for OpenAI and
  Google. This app keeps provider keys off the backend's environment on purpose, so every route
  that builds the graph without a run config had none. **Verified fixed**: background results come
  back, conversations open, switching between them works.
- 🟡 **A background worker guessed where it was** (§149) — `pd.read_csv('/data/…')`, then
  `/home/piero_linux/Mini-Me/…`, both exit 1, when the bare filename would have worked. A failed
  command now names the directory it ran in. **Awaiting a live run.**
- ✅ **A background worker's output is visible in the app** (§151) — **verified on a live run**:
  plots render in the transcript and the panel lists
  `<task>/guinea_pig_eda_output/plots/health_by_activity_box.png`. Its folder is created *inside*
  the conversation's, and the conversation it belongs to is remembered per thread because the run
  config is not visible at every construction site.
- 🟡 **A key per provider, for specialists on other providers.** Asked: *"what happens if I have
  an API from OpenAI, Google, Anthropic — do I have the ability to select the models for the
  subagents using independent API keys?"* **Half of it already works.** `ModelChoice.extra_keys`
  gathers one key per *other* provider any specialist was pointed at, reading `llm:<provider>`
  from the keychain, and `run_request_body` sends them all — the backend derives the same set
  from the specs (`models.py:117-122`), so a specialist on Anthropic while the coordinator is on
  OpenRouter is a supported shape today.
  **What is missing is the way to store the second key.** The Settings pane writes the key field
  to `llm:<currently selected provider>`, so filing an Anthropic key means selecting Anthropic,
  pasting, saving, and switching back — which nobody would guess, and §186's modal now interrupts
  each of those switches. A key list that shows every provider at once, with "stored"/"not set"
  per row, is the fix; the request path underneath needs nothing.

- ✅ **A finished background task is a button, not a sentence to retype** (§198). **Awaiting a
  live run.** Original note kept below for the reasoning.
- 🟡 *(was)* **A finished background task should be a button, not a sentence to retype.** Asked for
  directly: *"when a background task has a success, we should see a modal button that the user
  can press and that serves as a check status, so the user doesn't type it every time in the
  chatbox."* The app already knows a task finished — §31's Jobs panel polls for exactly that —
  so the result is one press away and is currently a question the researcher has to compose. The
  care needed: the button has to say *which* task, since several run at once (§43), and pressing
  it should open the result rather than sending a turn that asks for it.

- ⬜ **A loading state worth looking at.** Asked for after §177 and §178 put an honest one in
  place: *"maybe we can create a cool animation later for loading states."* What exists now is four
  braille frames and a sentence — correct, legible, and plainly a placeholder. The waits it covers
  are real and long: fifteen seconds of graph construction at launch (§176), a conversation
  opening, a turn that has not started streaming. Worth noting what the plain version already gets
  right, so a prettier one does not lose it: it never guesses how long the wait will be, it says
  *which* wait it is, and it appears in the place the result will appear. A skeleton of grey bars
  is the obvious upgrade and is the one to be careful with — it has to guess how many messages are
  coming and how tall each is, and guessing wrong makes the real transcript jump when it lands.

- ✅ **A file's own symbol, so the kind is readable at a glance** (§171). Asked for directly: *"add to the
  plan how we can put symbols of the files to know what is what. For example if its a python script
  the symbol of python must appear. its the same for json, etc etc etc."* Today `file_mark`
  (`main.rs`) returns one of four geometric glyphs — `▤` for anything tabular, `▩` for an image,
  `▦` for a PDF, `▤` again for everything else — so a `.py`, a `.json`, a `.log` and a `.txt` are
  all the same mark in the same colour. On a grid of tiles that is the only thing distinguishing
  them besides the name. Wanted: a per-extension mark for the kinds a research run actually
  produces — `.py`, `.ipynb`, `.json`, `.yaml`, `.md`, `.txt`, `.log`, `.html`, `.zip`, `.parquet`,
  `.db` — recognisable as *that* language or format rather than as a generic file. Note the
  constraint §12 already settled for the toolbar: a bundled SVG per kind is the honest route (PR #12
  builds exactly that machinery for the core controls, so this should reuse it rather than invent a
  second scheme), and a bare Unicode glyph is what we have because it needs no assets. Whichever
  way, `file_mark` is the one place it lands, and its colour should keep following the theme.
- ✅ **The Outputs panel does not survive a productive run** (§152 → §153 → §162). Twelve artifacts
  became twelve near-identical rows, each truncated to the 36 characters they *share*, and the
  transcript rendered every figure full width — ten plots, ten screens. Now: images in one group as
  a capped 2×2 grid whose fourth tile reads `+N`, everything else folder-grouped below it, and one
  click opens a modal with arrows, a `3 of 8` counter and a clickable filmstrip. Still open from
  this thread: **keyboard navigation in the modal** — arrow keys need a focus handle on the modal,
  because an unscoped binding would take the arrows away from the composer and a scoped one never
  fires from there (the §58/§84 trap).
- 🟡 **superseded** *(kept for the trail)* — **A background worker's output is visible in the app**
  (§151) — its folder is now created
  *inside* the conversation's rather than beside it, so `workspace::outputs` finds it by descending
  (§143) and shows `<task-id>/plot_yield.png` with the run that made it still legible. **Awaiting a
  live run.** Previously: The researcher's framing is the
  right one: *"the idea is to somehow view it in the app, not as a different folder outside the
  conversation folder."* §150 pinned the worker to the conversation's thread so its files land
  beside the conversation's, and on the run after that fix the Files panel still showed only
  `provenance.json` while the answer listed ten plots. **The pin is a means; the requirement is
  that a researcher sees the work without being told a path.** Two ways to satisfy it and they are
  not exclusive:
  - the worker writes into the conversation's folder (§150's pin), and
  - the app reads a finished task's own folder and folds it into Outputs, so a worker that lands
    anywhere is still visible.
  The second is the one that cannot silently fail, and it is not built.
- 🟡 **`execute` was told to prefer absolute paths, and sixteen files went to `/tmp`** (§160,
  §161). deepagents' own execute description says *"maintain your current working directory … by
  using absolute paths"* — sound inside a container the agent owns, and here `virtual_mode=False`
  means an absolute path is the researcher's real filesystem. The description is rewritten at
  import: the sentence is replaced, a rule naming the consequence is appended, and an upstream
  rewording is reported rather than silently failing. **Advice, not containment.**
  **Awaiting a live run.**
- ⬜ **`execute` can still write anywhere.** The rewrite above changes what the model is *told*.
  Real containment means the workspace is the only writable persistent mount — a bind-mount of
  `<work_dir>/tmp` over `/tmp`, or an isolated execution namespace. Not attempted, and explicitly
  not faked: pattern-matching a shell command for writes produces a containment claim that is
  false in every case nobody thought of.
- ✅ **The turn says files were saved without checking** (§175). An answer's filenames are now
  compared against the conversation's folder as outputs settle, and what is missing is said under
  the answer. Still a *report*, not a verdict: a file can be absent because the command failed,
  because it landed outside the workspace (§160), or because the answer invented it, and the app
  cannot tell those apart.
- ✅ **Conversations start already inside a project, and a deleted project comes back**
  (§154, §155, §166). Ordinary New starts at the workspace root; deletion waits for the backend
  and takes the folder with it; and §90's pre-tag migration no longer re-tags leftovers the moment
  the list is empty, which is what actually resurrected them.
- ⬜ **`start_async_task` accepts only `background_worker`**, by design (§114), while `/subagent`
  lists ten specialist names. Every researcher will reach for `exploratory_data_analysis` first, as
  this one did twice. Either the tool description says so, or it routes.

**Next**
- ✅ **Outputs a turn wrote into a folder** (§117, §143) — the panel now descends through named
  output folders with explicit depth/file bounds, keeps the relative path visible, skips tool
  caches, and says when the bounded view omitted anything.
- ✅ **The filter field** (§92, §97, §99) — measured rather than guessed: 0.0px in the popup
  against 204 and 533 elsewhere. One link in the chain stated no width; confirmed fixed on a real
  window.
- 🟡 **Nine upstream reports** — written, in `docs/upstream/`, each with evidence and a suggested
  fix. Two have since landed as Mini-Me PRs (the corpus id and the skills path). The two
  `langgraph_runtime_inmem` ones are silent data loss and affect anyone running `langgraph dev`;
  filing those is an outward-facing act on someone else's repository, and the decision is not
  this repo's.

**The literature path** *(new — §119–§139)*
- ✅ **Citations are built in code** from the publisher's record, not composed from the model's
  memory. Verified against Crossref on 17 papers across 6 fields: 17/17 DOIs resolve, 17/17
  titles, 17/17 years, 0 contradictions.
- ✅ **The subagent searches before it can answer** (§133). Its structured response was bound as a
  tool and `tool_choice` forced, so answering from memory in one step was the cheapest legal move
  it had. Prompts had been arguing with that for four days.
- ✅ **Nothing retrieved is dropped** (§137) — a run that found 24 papers reported 9; the rest are
  appended where the list leaves the backend.
- ✅ **The backend log says which commit produced it** (§134) and its diagnostics distinguish
  success from failure (§132), which four nights of misdiagnosis said they had to.
- ✅ **A report with citations downloads as a PDF** (§141). It had never been tried with a source
  in the list: this app sent bare citation strings and the route reads `source.get("citation")`, so
  the first attempt was a 502 on the whole PDF over the reference list alone. Fixed on both sides —
  and sending the object sends the `link` the client had held and dropped since §91, so a rendered
  bibliography now resolves.
- ✅ **Papers the model adds from memory are marked** (§185). Barrera et al. (2016) came back
  real and relevant from a journal Semantic Scholar indexes poorly — much of CIP's own literature
  looks like that — and sat beside record-backed citations looking identical, because the panel
  reported *errors* and silence meant both "nothing wrong" and "nothing checked". `Origin` is now
  a separate question from `Verdict`: the header counts what is unverified, the row says which,
  and an exported `.bib` carries the same note into Zotero. **Awaiting a live run.**
- ⬜ **A paper with no DOI and no corpus id** is invisible to the "never reported" accounting
  (7 returned, 6 recorded, on 2026-08-09). Backend, not client: `_papers()` in
  `overlay/minime_local/sources.py` is keyed on the identifiers such a paper does not have, so it
  is missing from the set the count is taken against. §185 makes the *client* say what is
  unverified; this is the other half, and it is a log line rather than a screen.
- ✅ **`dataverse_explorer` searches, and reads what it found, before it can recommend** (§142,
  Mini-Me PR #45). **Verified on a live run**: both gates fired, and all four persistent ids
  resolved to real CIP datasets on late blight clone trials in Peru. The filename it was asked for
  in capitals was wrong **9 times out of 9** and silently corrected each time — before this, every
  search wrote to a different file and every read looked for one that did not exist.
- ⬜ **Seven subagents still hold their invariants in prompts** (§140). Each carries a
  `response_format` and so each has the §133 exit. `hypothesis_generator` is next — it emits
  citations too, so it can reproduce the §138 bug somewhere nobody would think to look. The
  mechanism is a base class now (`middleware/tool_gate.py`), so each costs a `steps` tuple.
- ✅ **~10s of every startup** was an Asta token minted fresh with no validity check
  (`backend.rs`, §131). `mint_asta_token` now tries the keychain, then the CLI's own cache,
  and checks `exp` before paying for a network mint — so the seconds are spent once a week
  rather than once a launch. **Awaiting a stopwatch**, which is the only thing that settles it.
- ⬜ **`setup-wsl.sh` leaves a checkout with every file modified** from line endings, which breaks
  any git operation on it.
- ⬜ **Publish `v0.1.0`** — tagged and built; the draft needs a decision about who may have it.
- ⬜ **A custom store** (§93) — deliberately after the checkpointer, and only with numbers:
  alpha API, and replacing it means owning semantic search and TTL.
- ⬜ **A native Windows backend?** (§95) — the case is the WSL install, not storage. Its own
  experiment: run a real turn without the distro and see what actually breaks.

**Proposed — P7**
- ✅ **The provenance record** — done (§73–§75 designed it, §83 built it, §85–§86 fixed it on
  real data): a `Record` written to `provenance.json` in the thread's own directory as each turn
  finishes, and a modal with two views — a **timeline** of bars on one shared scale, and the
  **graph**, drawn with `canvas` and `PathBuilder`. Edges are causal where the namespace path
  says so (`delegated to`) and observed where only arrival does (`then`), and the modal says
  which is which. Cycles across turns, a tree within one. The chain-of-chips §73 proposed as the
  first stage was built, shown, and rejected in one screenshot — *"the other image its not a
  graph"* — which is what the staging was for.
- ✅ **`/subagent` slash commands** — done (§76–§81): a registry captured from the coordinator
  as it is assembled, a `/` picker over the real ten specialists, name validation that suggests
  the nearest match, and background dispatch from the palette. The one thing no test in this
  repo could settle — whether the coordinator *honours* "delegate this to `X`" — is
  **confirmed** on a real turn: `/academic_researcher search deseq2 paper` delegated, and the
  transcript carries the specialist's own trace (`academic_researcher · 4 steps · 1879 chars`)
  rather than a coordinator answer.

**Deliberate deferrals**
- ⬜ **Old workspaces are not migrated** (§42). Threads from before §51's tag **do** appear
  again: §90 found them filtered rather than lost, and §91 fixed the repair after measuring that
  the first attempt recovered 1 of 26.
- ⬜ **Async subagents stay opt-in**, on a preview deepagents API.

**A correction, since it changes what is possible.** This document said text selection was
"the one thing the framework genuinely makes hard — GPUI 0.2.2 cannot." That is **wrong**:
`TextLayout::index_for_position` exists (`gpui-0.2.2/src/elements/text.rs:483`, and
`line_layout.rs:283`), and §55's multi-line composer already built per-line hit-testing and
per-line selection quads against it. Checked, not assumed — which is the fourth time in this
project a confident claim about the artefact dissolved on one `grep` (§52 has the other
three).

**Owed upstream** (found here, belongs in Mini-Me)
- ⬜ `guardrails.py` claims sandbox isolation host execution does not provide (§18).
- ⬜ The theorizer reports a *guess* instead of the command's real output (§35) — seven
  rounds, the most expensive defect of this project.
- ⬜ `deepagents`' `start_async_task` passes no config, so no self-hosted deployment can
  give a background run its model, key or recursion limit (§38/§39).
- ⬜ `agent.py`'s `make_backend` docstring says the `langgraph dev` store "loses content on
  process restart" (§82). It does not — the dev runtime's store is disk-backed. A docstring,
  but a load-bearing one: this app tells researchers to restart the backend.

**Health of the bet.** The two risks that could have killed this are both down:
**R1** (GPUI as an unstable `git` dep) — GPUI is a *published* crate, pinned at
`gpui 0.2.2`. **R2** (API churn) — the P6.0 sketch compiled against it unchanged.
What remains is scope risk (**R3**: rebuilding rich UI) and packaging (**R4**) —
work, not uncertainty. **R4 shrank** once the target became local-first for
colleagues rather than a notarized public installer.

## What this product is (clarified 2026-07-30)

A **local-first, single-user research workbench** — deliberately *not* a hosted
service. The web app is the thing we are leaving behind, so the desktop app should
shed its infrastructure rather than reproduce it:

> **Windows is the primary platform: ~98% of our users are on Windows**
> (stated 2026-07-30). Linux is the *development* platform, not the target. Every
> feature is only "done" once it works on Windows, and anything that assumes a
> POSIX shell is a defect for almost the whole user base — see §13.

- **Drop the hosted services.** No **WorkOS** (auth is meaningless for a local
  single user) and no **LangSmith** (sandbox *and* tracing). §11 proves both are
  droppable — WorkOS for free today, LangSmith once execution is local.
- **Execution runs on the user's machine** (§10). That is also what makes an
  installable app possible: you cannot ask every scientist to provision their own
  remote sandbox.
- **The user's own API keys, on their own computer** — OS keychain, plus a setup
  tutorial. Two externals remain by nature: **Asta** and the **model API**.
- **"Click to update"**, Zed-style. The backend is Python, so an update is a fetch
  + dependency sync of a pinned checkout — no compile step. (Self-updating the
  Rust binary is a separate, later problem.)
- **Mini-Me stays upstream, unmodified and pinned** — bundled, never forked. The
  agent stack *is* the product and is actively developed; a modified copy would
  either accrue permanent merge debt or freeze and drift from the web app. Desktop
  needs are met by one opt-in seam (§10), not a fork.

---

## 1. Why desktop, why GPUI

**Why desktop.** The web app's ceiling is the browser sandbox. A desktop client
unlocks: local filesystem + native file dialogs (drop a CSV, no upload dance),
long-running/background agent jobs as first-class OS processes, offline, OS
keychain for secrets, multi-window, and a fast keyboard-driven multi-pane UX.

**The token win.** Mini-Me's deployed backend can't auto-refresh the Asta token
(it expires ~weekly; PR #33 added a manual paste-and-store workaround). The
**local** `asta` CLI *does* auto-refresh. Running the backend as a local sidecar
means the desktop app inherits that — **the token-expiry pain disappears.**

**Why GPUI (the chosen direction).** The goal is to "copy the best from Zed": a
fast, native, GPU-rendered, keyboard-first workbench. GPUI is the framework that
makes Zed feel the way it does. This is the high-ceiling, high-effort path
(Tauri-wrapping the existing React app is the lower-risk fallback documented in
Mini-Me's `docs/asta-integration-plan.md`, Phase 6). We proceed on GPUI with eyes
open — see the risk register.

**What stays the same.** Agents are the product and do **not** get rewritten. The
desktop app speaks to them over the existing HTTP/stream protocol. Org policy
stays **human-gated**: nothing auto-runs.

---

## 2. Honest risk register (read before building)

| # | Risk | Severity | Mitigation / kill-criterion |
|---|------|----------|------------------------------|
| R1 | ~~**GPUI is not a stable published crate.**~~ **Resolved (P6.1):** GPUI *is* published to crates.io. `gpui 0.2.2` is self-contained (companions `gpui_macros`/`gpui_util` published too; no `git`/`path` deps), so no Zed-monorepo `git` dependency is needed. | ~~High~~ **Low** | Pin the published crate: `gpui = "=0.2.2"`. Bump deliberately. Zed `git` rev `00bd72e…` (v1.13.1) kept documented as a fallback if a newer API is ever required. |
| R2 | **GPUI API churn.** Examples online drift from the current API (`App`/`AppContext`/`Context`, `cx.new` vs `cx.new_view`, `Render` signature). | Med | Build against the `examples/` in the pinned Zed rev, not blog posts. The `crates/app/src/main.rs` here is a *starting sketch* to reconcile against that rev. |
| R3 | **Rewriting rich UI** (streaming markdown, artifacts panel, PDF/figure views, charts) in GPUI is a lot of surface the browser gave for free. | High | Port incrementally (P6.3). Start with plain text + a simple list; add markdown/artifacts later. Consider embedding a webview *per-panel* only if a surface proves impractical in GPUI. |
| R4 | **Sidecar packaging.** Bundling a Python backend (uv/venv + the `asta` CLI + system deps) into a shippable app is non-trivial per-OS. | Med | P6.2 spawns a *dev* sidecar (assume `uv`/venv on PATH). Packaging (PyInstaller / uv bundle / container) is a later milestone, not MVP. |
| R5 | **Linux GPU stack variance** (Vulkan/Wayland/X11). | Low-Med | GPUI supports Linux via `blade`. Confirmed the dev machine has `libvulkan/libwayland/libxkbcommon/libX11`. Test early on the target machine. |
| R6 | **Team Rust capacity.** The rewrite needs sustained Rust work. | Med | Confirm before P6.1. This is an organizational, not technical, gate. |

**Overall:** the direction is viable but front-loaded with framework risk. P6.1
(a buildable window) is the go/no-go gate before any real investment.

---

## 3. Architecture

```
┌─────────────────────────────────────────────────────────┐
│  mini-me-desktop  (Rust / GPUI)                          │
│                                                          │
│  ┌───────────────┐   ┌──────────────────────────────┐   │
│  │  UI (GPUI)    │   │  BackendSupervisor            │   │
│  │  - chat pane  │◄──┤  - spawns the Python sidecar  │   │
│  │  - artifacts  │   │  - health-check / restart     │   │
│  │  - spine/plan │   │  - streams turns over HTTP/SSE│   │
│  │  - cmd palette│   └───────────────┬──────────────┘   │
│  └───────────────┘                   │ localhost:PORT    │
└──────────────────────────────────────┼──────────────────┘
                                        │
                   ┌────────────────────▼────────────────────┐
                   │  Mini-Me backend (Python, unchanged)     │
                   │  coordinator + subagents + skills        │
                   │  local `asta` CLI (auto-refreshing auth) │
                   └──────────────────────────────────────────┘
```

- **Client ↔ backend boundary:** the existing HTTP + streaming protocol the web
  frontend already uses (LangGraph run/stream). The desktop app is *another
  client* of that protocol — no new agent code.
- **Local sidecar:** `BackendSupervisor` spawns the backend (e.g. `uv run …` or
  the LangGraph dev server) on a localhost port, waits for health, and tears it
  down on quit. Auth uses the local `asta` CLI's own refreshing token.
- **Secrets:** model/API keys and any Asta token go in the **OS keychain** (via
  the `keyring` crate), never a plaintext dotfile.

---

## 4. Crate / workspace layout

```
mini-me-desktop/
├── Cargo.toml               # workspace
├── rust-toolchain.toml      # pinned toolchain
├── .gitignore               # /target, etc.
├── README.md
├── docs/
│   └── desktop-app-plan.md  # this file
└── crates/
    └── app/                 # the desktop binary
        ├── Cargo.toml       # gpui (git, pinned), serde, tokio, keyring…
        └── src/
            ├── main.rs      # GPUI app entry + root workbench view (sketch)
            └── backend.rs    # BackendSupervisor: spawn/health/stream (stub)
```

Future crates as the app grows: `protocol` (typed request/response mirrored from
the backend), `ui` (reusable GPUI components), `sidecar` (packaging).

---

## 5. Milestones

- ✅ **P6.0 — Spike doc + skeleton.** Plan + Cargo workspace + a root view sketch
  + a sidecar-supervisor stub. Authored without a Rust toolchain, so unverified.
- ✅ **P6.1 — "Hello workbench" (go/no-go).** Pin `gpui`, get **one window** on
  screen, reconcile `main.rs` against the pinned API. *Kill-criterion R1 — passed;
  see §8.* (The command palette slipped to P6.3 — the gate was the window.)
- ✅ **P6.2 — Talk to the real backend.** `BackendSupervisor` spawns the Python
  sidecar, health-checks it, and streams **one real coordinator turn** end to end,
  rendering assistant text as it arrives. *Verified on Windows 2026-07-30 — the
  coordinator answered in the chat pane, status `done`.*
- 🔴 **P6.2.5 — Local-first backend** *(new; critical path — §10/§11).* Replace the
  remote LangSmith sandbox with host execution (`LocalShellBackend`) behind
  `MINIME_EXECUTION_BACKEND`, add the ~6 bespoke methods deepagents lacks
  (`aget_work_dir`, `aexecute_untruncated`, the lifecycle quartet,
  `_emit_sandbox_status`), and stop configuring WorkOS/LangSmith. Revisit
  `prompts.py` (path + `python3` rules) and `guardrails.py` (the isolation
  assumption), and gate `execute` with human approval.
  *Acceptance:* a real turn — including an `asta` subagent call — completes with
  **no `LANGSMITH_API_KEY` and no `WORKOS_*`**, executing on the host.
- 🟡 **P6.3 — Port the core panels.** In progress, in this order:
  1. ✅ **Composer + transcript scroll** — a real text field (type, Enter sends)
     and a scrollable transcript. §12.
  2. ✅ **Project spine** — the right panel now renders live `GET /project` data
     (mission, completed, pending, suggestions) instead of a hardcoded string. It
     refreshes on launch and after every turn, since the mission is derived from
     the first question. Clicking a suggestion **loads its prompt into the
     composer** — it never runs it, keeping the human gate. The headless
     `--check-backend` now covers this route too, so a decode regression shows up
     as a failed check rather than a silently empty panel.
  3. ✅ **Artifacts/Outputs** — an OUTPUTS section under the spine, fed by the
     `values` stream event so it fills in *during* a turn. Buckets come from the
     live payload: `datasets, sources, reports, files, hypotheses, libraries,
     analyses` (`edges` is graph wiring, not an output, so it's hidden). Each shows
     a count plus up to four titles, then "+N more" — a literature search can
     return dozens. Labels fall back through `title → name → filename → label →
     question → id`, and an unlabelled item is still *counted* rather than dropped.
     *Two corrections to what this plan previously assumed:* the state key is
     `files`, not `todos`; and **`GET /files/{thread_id}` is a download route**
     (it 400s with `missing 'path' query param`), not a listing — so artifacts come
     from the stream, not that route.
  4. ✅ **`sandbox_status`** from `custom` events now drives the status line
     (`Creating sandbox… → Sandbox ready`). This matters because the first turn on a
     cold thread blocks on that provisioning, and without it the UI looks stuck.
  5. ✅ **Command palette** — Zed-style `ctrl-p`/`cmd-p` (§17): a ranked, filterable
     list of seven commands over the workbench. Building it surfaced a real defect —
     "New thread" was meaningless because *every* turn created a new thread — so
     conversation continuity landed with it.
  6. ✅ **Agent activity trace** (§15/§15c) — a delegated turn is no longer silent.
     `stream_subgraphs: true` is now requested, subagent frames are attributed by
     namespace and named from `lc_agent_name`, and the transcript shows the
     coordinator's delegation plus a collapsible group per subagent with its tool
     calls and streamed text. *Verified live 2026-07-31.*
- ✅ **P6.4a — Settings panel + keychain secrets** (§20/§22/§22b). `ctrl-,`, two stores
  (`settings.toml` for settings, the OS keychain for keys), secrets delivered to the
  sidecar as environment variables so the checkout's `.env` becomes optional, and a
  first-run panel instead of a failed turn. *Gates the installable.*
- ⬜ **P6.4b — Native affordances + shipping.** Local file → analysis,
  background-run tray + notifications, **keychain-stored keys**, multi-window.
  Plus what "installable" now means: a **pinned Mini-Me checkout + venv the app
  provisions**, a **"click to update"** button, a **setup tutorial**, and Windows
  process-tree teardown via a Job Object (§9). Not a notarized public installer —
  a guided local install for colleagues.

**MVP acceptance:** a launchable app that opens a project, runs a real coordinator
turn against the local sidecar, streams the answer, renders the artifacts/spine
panels, and does **one** thing the web app can't (local file → analysis, or a
background-run notification).

---

## 6. Decisions

**Locked (2026-07-29):**

- **Repo shape:** ✅ **separate repo** — `mini-me-desktop`, this one. Published
  private at `CENTRO-INTERNACIONAL-DE-LA-PAPA/mini-me-desktop` (2026-07-29).
- **Backend locality:** ✅ **local sidecar** — the client spawns the Python backend
  on localhost. (Nuance found in P6.2: this removes the web app's paste-a-token
  dance, but the backend forwards a *pre-minted* `ASTA_TOKEN` rather than
  refreshing live — see §9.)
- **UI framework:** ✅ **Rust on GPUI**, pinned to published **`gpui = "=0.2.2"`**
  from crates.io — *not* a Zed monorepo `git` rev (§8). Tauri remains the
  documented fallback but was not needed.
- **Agents stay Python/TS:** ✅ no agent code is rewritten; the desktop app is
  another client of the existing HTTP/SSE protocol.
- **Where the app + sidecar run:** ✅ **co-located on Linux** for development
  (the checkout, `.env`, and `asta` CLI live there). The app itself also builds
  and runs on Windows.

**Locked (2026-07-30):**

- **Product shape:** ✅ **local-first, single-user**; not a hosted service, not
  production. This is the premise the rest follows from.
- **Execution locality:** ✅ **local host execution** (§10), proven to be the only
  blocker to dropping LangSmith (§11).
- **Hosted services:** ✅ **drop WorkOS and LangSmith** (sandbox + tracing).
- **Mini-Me:** ✅ **bundled upstream, pinned, unmodified — not forked.** Desktop
  needs are met through one opt-in seam.
- **Secrets:** ✅ the user's own keys, in the **OS keychain**, with a setup tutorial.

**Locked (2026-07-31):**

- **Execution default:** ✅ **host, with `execute` human-gated** (§19). The remote
  sandbox stays reachable via `--sandbox` but nothing uses it by default.
- **Where the §10 change lands:** ✅ **a `PYTHONPATH` overlay in this repo**
  (`overlay/`), not a PR or a fork — the checkout stays byte-for-byte upstream, which
  is what "bundled, pinned, unmodified" asks for. An upstream seam remains the nicer
  destination and the code would move across almost verbatim (§18).

**Open:**

- **Approval fatigue:** whether a long analysis holding a dozen commands is tolerable.
  If not, the answer is remembered decisions, not removing the gate (§19).
- **Human-gating `execute`:** approval UX for host commands (policy + design).
- **Rust capacity:** an organizational gate (R6) — sustained Rust availability.
- **`asta` version pinning on the host:** the sandbox pinned `v0.101.0`; the dev
  box has `0.101.1`. Needs a version check at startup.

---

## 7. Build & run

**Prereqs (Linux / Ubuntu 22.04):** rustup + the stable toolchain, plus the GPUI
system dev headers:

```bash
sudo apt-get install -y libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev \
                        libasound2-dev libvulkan-dev
```

(Already on the dev box: `libx11`/`libxcb`/`fontconfig`/`freetype`/`openssl`/`zlib`
plus a C toolchain + `cmake`. `protoc` is **not** required — we depend only on
`package = "gpui"`, not Zed's proto crates.)

```bash
cd mini-me-desktop
cargo build -p mini-me-desktop-app   # verified green: rustc 1.97.1, gpui 0.2.2
cargo run   -p mini-me-desktop-app   # opens the workbench window (needs a display)
```

`cargo build` is confirmed working (P6.1). `cargo run` must be launched from a
graphical session (Wayland/X11 + a Vulkan device) — it cannot open a window from a
headless TTY. On **Windows** GPUI renders via DirectX, so no Vulkan/Wayland is
needed and `cargo build && cargo run` works natively.

### Backend prerequisites (the sidecar)

The app spawns the Mini-Me Python backend, so that checkout must be able to serve:

```bash
git clone <Mini-Me>            # then, inside it:
uv sync --extra dev            # NOT plain `uv sync` — see below
```

**`--extra dev` is required.** The LangGraph *CLI* lives in an optional extra
(`langgraph-cli[inmem]` under `[project.optional-dependencies] dev`), which plain
`uv sync` skips. You then get the server libraries but **no `langgraph` entry
point**, and both `langgraph dev` and `uv run langgraph dev` fail with "program not
found" (hit on Windows 2026-07-30). The supervisor's spawn error now names this fix.

The checkout also needs a populated `.env` — at minimum `OPENAI_API_KEY`, plus
`ASTA_API_KEY` / `ASTA_TOKEN` for Asta features and, **until P6.2.5 lands**,
`LANGSMITH_API_KEY` (§11 explains why the run dies without it).

How the app finds the checkout: `MINIME_BACKEND_DIR` wins; otherwise it tries
`~/Documents/Mini-Me` and `~/Documents/GitHub/Mini-Me` (honouring `USERPROFILE` on
Windows) and then `../Mini-Me`. Related env vars: `MINIME_BACKEND_PORT`,
`MINIME_BACKEND_URL`, and `MINIME_BACKEND_ATTACH_ONLY` (never spawn — talk to a
backend you started yourself).

---

## 8. P6.1 execution log (2026-07-29)

The go/no-go gate. **Outcome: PASS on build.** `cargo build -p mini-me-desktop-app`
succeeds; the visual window-check is the user's remaining step (the build shell is
a headless TTY, so it can compile but not display).

**Key finding — GPUI is published.** The P6.0 assumption ("not on crates.io, must
be a Zed `git` dependency") was wrong. `gpui 0.2.2` is on crates.io (updated
2025-10-22), fully self-contained — no `git`/`path` deps, and its only companions,
`gpui_macros` and `gpui_util`, are published at the same version. We therefore pin
**`gpui = "=0.2.2"`**, which retires most of risk **R1** (no unstable monorepo
`git` dep). This Oct-2025 published snapshot still exposes the classic
`Application::new().run()` entry point, matching the scaffold. Newer Zed revs
(e.g. `v1.13.1` = `00bd72e7838f4b875a913cd112b47a0ebe1ca62b`) have since moved the
entry point into a separate `gpui_platform::application()` crate — kept documented
as the fallback if a newer API is ever needed.

**API reconciliation — zero code changes required.** The P6.0 `main.rs` sketch
compiled against `gpui 0.2.2` unmodified. Cross-checked against the crate's own
`examples/hello_world.rs`:
- `Application::new().run(|cx: &mut App| …)` ✓
- `cx.open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), .. }, |_, cx| cx.new(|_| …))` ✓
- `Render::render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement` ✓
- `Bounds::centered(None, size(px(w), px(h)), cx)`, `rgb(u32)`, and the
  macro-generated Tailwind-style helpers (`p_*`, `gap_*`, `w`, `h_full`,
  `size_full`, `border_r_1`, `flex_grow`, …) ✓

**Toolchain.** rustc/cargo **1.97.1** (stable), via rustup. Workspace
`rust-toolchain.toml` stays `channel = "stable"`. (gpui's own repo pins 1.95.0, but
building only the `gpui` crate downstream is fine on newer stable.)

**Linux system deps.** Missing on the dev box and installed for the build:
`libwayland-dev`, `libxkbcommon-dev`, `libxkbcommon-x11-dev`, `libasound2-dev`,
`libvulkan-dev`. Already present: X11/xcb/fontconfig/freetype/openssl/zlib + C
toolchain + cmake.

**Build result.** `cargo fetch` resolves the full graph with no conflicts;
`cargo build` finishes green (~1m35s cold on a 32-core box). One benign note: an
upstream future-incompat warning in `proc-macro-error2` (transitive; not our code).
The `BackendSupervisor` dead-code warnings are silenced with a documented
`#![allow(dead_code)]` — it's P6.2 scaffolding, constructed but not yet wired.

**P6.1 CLOSED (2026-07-30).** *(P6.2's log is §9.)* Visual confirmation done: `cargo run` on **Windows**
(GPUI's DirectX backend) opened the three-pane workbench window — orange-accented
rail, chat pane with the two placeholder turns, and the right panel with the
mission + P6.3 note — exactly as designed. Note the run environment: the app
**builds on Linux (headless)** and **runs/renders on a Windows dev machine**
(`C:\Users\LENOVO\…\mini-me-desktop`); Windows is a first-class GPUI target
(DirectX — no Vulkan/Wayland needed). **Go decision: proceed to P6.2.**

---

## 9. P6.2 — talk to the real backend (in progress, 2026-07-30)

**Decision: app + sidecar co-located on Linux**, where the Mini-Me checkout, the
`.env` keys, and the `asta` CLI already live. (The app also builds and runs on
Windows, but the backend/secrets are on the Linux box; keeping them together is
the true local-sidecar shape.) Verified present: `uv 0.9.28`, Python 3.12.2,
`asta 0.101.1`, and `.env` with `OPENAI_API_KEY`, `LANGSMITH_API_KEY`,
`ASTA_API_KEY`, `ASTA_TOKEN`, `DEEP_ATD_RUNTIME_MODE`.

### The protocol, as mapped from the Mini-Me repo

The backend is a **LangGraph server**; the desktop app is just another client of
the protocol the React frontend already speaks. No agent code is duplicated.

| Concern | Contract |
|---|---|
| Launch | `uv run langgraph dev --host 127.0.0.1 --port 2024 --no-reload` (cwd = Mini-Me repo; auto-loads `.env`; no browser) |
| Graph id | `assistant_id = "agent"` (`langgraph.json`) |
| Health | `GET /ok` → `200 {"ok":true}` — the P6.0 stub's guess was right |
| New thread | `POST /threads` `{}` → `{"thread_id": …}` |
| Run | `POST /threads/{id}/runs/stream`, `Accept: text/event-stream`, body `{assistant_id, input:{messages:[{type:"human",content}]}, stream_mode:["messages-tuple"]}` |
| Tokens | SSE `event: messages` → `data: [chunk, meta]`; append `chunk.content` where `chunk.type == "AIMessageChunk"` (content is a string *or* typed blocks) |
| Auth | **none needed in local dev** (`backend/auth.py` admits an unauthenticated `local-user`); the model falls back to `OPENAI_API_KEY` from `.env` |

Deliberate simplification: we leave `stream_subgraphs` off, so only *coordinator*
tokens arrive — no subagent namespaces to filter. Subagent streams, `values`
(state), and `custom` (`sandbox_status`) events are P6.3 material.

> ⚠️ **`messages-tuple`, not `messages`.** Asking for `stream_mode:["messages"]`
> looks right and fails silently with **zero tokens**: the server then takes its
> v1 path and emits `messages/partial` / `messages/complete` frames with a
> different payload shape. Only `messages-tuple` is rewritten into `event:
> messages` frames carrying `[chunk, metadata]` tuples
> (`langgraph_api/stream.py:231-233, 345-350`). Cost us one debugging cycle in
> P6.2; there is now a unit test pinning the request body.

**Correction to the north-star premise.** The "local `asta` auto-refresh" win is
real but indirect: the backend does **not** invoke `asta` per request. It reads a
pre-minted `ASTA_TOKEN` from `.env` (minted locally once via
`asta auth print-token --raw --refresh`) and forwards it to the remote LangSmith
sandbox where subagents actually execute. So locality removes the web app's
paste-a-token dance, but it is not a live token refresh. A plain coordinator turn
needs only `OPENAI_API_KEY`.

### What was built

- **`crates/app/src/protocol.rs`** — typed LangGraph client (`create_thread`,
  `stream_turn`, `is_healthy`) plus an **incremental SSE decoder**. Network chunks
  split anywhere, so bytes are buffered until a `\n\n` (or CRLF) terminator; a
  unit test feeds a stream **one byte at a time** to prove reassembly. 6 tests
  cover byte-split framing, string vs. block `content`, non-assistant chunks,
  subagent-namespaced events, and error events.
- **`crates/app/src/backend.rs`** — the stub became a real supervisor:
  attach-or-spawn (`ensure_running` attaches to an already-running backend
  instead of double-spawning), `/ok` polling that **fails fast if the child
  exits** rather than waiting out the budget, inherited stdio (no pipe to
  deadlock on), and repo-path resolution via `MINIME_BACKEND_DIR` → conventional
  locations. Kills the child on drop.
- **`crates/app/src/sidecar.rs`** — the async↔UI bridge. GPUI has its own
  executor and `reqwest` needs Tokio, so instead of mixing runtimes we keep a
  Tokio runtime here and hand events back over a `futures` channel (which is
  executor-agnostic, so GPUI awaits it directly). The runtime and child outlive
  individual turns, so ending a turn never kills the backend.
- **`crates/app/src/main.rs`** — streaming UI: tokens append live to the
  transcript via `cx.spawn` + `weak.update(…)` + `cx.notify()`, with a status bar
  (backend state / errors / base URL) and a Run button. A text composer is P6.3;
  P6.2 uses one seeded prompt.
- **`--check-backend [--stream | --prompt "…"]`** — a headless self-check that
  exercises spawn → health → thread → stream with **no window**, so the contract is
  testable on a headless machine (and doubles as a debug tool). `--prompt` runs an
  arbitrary turn and reports the activity trace (§15c).
- **`--replay <capture>`** — decodes a saved SSE capture into the transcript it would
  produce. No backend, no window, no tokens (§15c).

**Env overrides:** `MINIME_BACKEND_DIR`, `MINIME_BACKEND_PORT`,
`MINIME_BACKEND_URL`, `MINIME_BACKEND_ATTACH_ONLY`.

### Bugs the live run caught (all fixed)

Running against the real backend — not just compiling — found three defects the
type system could never have:

1. **Orphaned backend.** `uv run langgraph dev` *forks* the real server, so
   `Child::kill()` reaped the wrapper and left `langgraph dev` holding port 2024
   (reparented to init). Fixed two ways: prefer the checkout's
   `.venv/bin/langgraph` entry point (a single process we actually own), and put
   the child in **its own process group**, signalling the whole group (SIGTERM,
   then SIGKILL) on drop.
2. **`std::process::exit` skips destructors** — the `--check-backend` failure path
   leaked the backend for exactly that reason. The sidecar is now dropped
   *before* exiting.
3. **The browser hijack.** `langgraph dev` opens LangSmith Studio by default
   ("🎨 Opening Studio in your browser…"). A client shouldn't seize the user's
   browser: we pass `--no-browser`.

Also: piping the child's stdio to *us* meant the child held our stdout open (and
risked deadlocking on a full pipe buffer). Its logs now go to
`/tmp/mini-me-desktop-backend.log`, which the UI cites in error messages.

### Status: P6.2 backend path VERIFIED end to end (2026-07-30)

`cargo build` green · `cargo test` **7/7** · `cargo clippy` clean (only an
upstream `proc-macro-error2` note). Against the live sidecar:

```
health   : ok (sidecar started)     # spawned the venv binary; healthy in ~2s
thread   : 019fb3cb-4be8-…          # POST /threads
stream   : 75 chunk(s), 423 chars   # a real coordinator turn, streamed
backend check: PASS                 # and no orphaned process afterwards
```

**Remaining for P6.2:** the visual check — `cargo run` in a graphical session and
confirm tokens land in the chat pane live. (Unrelated observation, *not ours to
fix*: the backend prints a non-fatal `yaml.scanner.ScannerError` from a skill
docstring during startup; the server boots fine. Mini-Me is read-only here.)

---

## 10. Execution locality: remote sandbox → local host

**Decided (2026-07-30): go local.** The product is a **local-first, single-user
workbench** — not a hosted service. That makes the remote **LangSmith sandbox**
(and WorkOS auth) infrastructure we neither need nor want: it costs a per-user
API key, a cold start, a 10-minute idle TTL, a 1-concurrent-sandbox free tier, and
it ships the user's files to someone else's VM. Execution moves to the host via
deepagents' `LocalShellBackend`. **This is now on the critical path** — see §11 for
the experiment that proves it is the *only* thing standing in the way.

> **Resolved 2026-07-31 (§18):** it landed as a `PYTHONPATH` overlay in *this* repo.
> The Mini-Me checkout is not modified, so it remains read-only reference in fact and
> not just in intent.

### What the codebase says (read-only audit, 2026-07-30)

The good news: **the seam is narrow and the replacement already exists.**

- Mini-Me depends on `deepagents 0.6.1`, which **already ships**
  `LocalShellBackend(root_dir=…, virtual_mode=True)`. Critically it subclasses
  `FilesystemBackend` *and* `SandboxBackendProtocol`, so `supports_execution`
  stays true and the `execute` tool is **not** stripped from the agent/subagents.
  Its `virtual_mode` path-rooting is almost exactly the semantics
  `sandbox.py`'s `_resolve_for_read/_write` hand-rolls today.
- The LangSmith SDK is imported in **one** module (`backend/sandbox.py`), and the
  injection point is **~3 lines**: `agent.py:86` (construct), `routes/common.py:41`
  (HTTP routes), with `runtime.py:50`'s ContextVar deliberately typed `Any`.
- Every tool module is already **duck-typed** against the backend surface
  (`getattr(sandbox, "aexecute_untruncated", None) or sandbox.aexecute`), and the
  test suite already substitutes fake sandboxes — so a swap is a proven pattern.
- `/skills/` and `/memories/` are already routed to `StoreBackend` via
  `CompositeBackend`; only the `default` route is the sandbox.
- Report rendering (`pypandoc` + `typst`) **already runs host-side**.

The tail that would have to be written — deepagents has no equivalent: a
`aget_work_dir()` (7 call sites, trivially `root_dir`), `aexecute_untruncated()`
(without it the ~500 KB theorizer record gets clipped to unparseable JSON), the
lifecycle quartet `aresolve`/`try_resolve`/`aresume`/`adelete` (locally mostly
no-ops or `mkdir`/`rmtree`), and `_emit_sandbox_status` (emit `ready` at once, or
the UI waits on a state that never comes). Keep the output-truncation cap — it
protects the UI from verbose PyMC/sklearn output, and is *not* sandbox-specific.

### The trade

**Wins (real, and aligned with why we went desktop):** no cold-start
provisioning; no LangSmith dependency for the filesystem (only for tracing); no
free-tier **1-concurrent-sandbox** limit; no 10-min idle TTL; and true local files
— the "no upload dance" promise.

**Costs:**

1. **Isolation disappears.** `guardrails.py` states the current design *relies on
   sandbox isolation* for the execution backend. `virtual_mode` constrains the
   filesystem *tools*; it does **not** constrain what a shell command the model
   wrote can reach. For a desktop app running the user's own code on the user's
   own machine that may be an acceptable trade — but it is a **product decision**,
   and `guardrails.py` plus CIP's human-gated policy must be revisited with it
   (deepagents explicitly recommends HITL for this backend).
2. **Host prerequisites.** `asta` must be on PATH at the pinned version (the
   snapshot pins `v0.101.0`; the dev box has `0.101.1`), plus a `python3` with the
   numerical stack — note that's a *different* interpreter from the backend venv
   unless we deliberately point `env`/PATH at one. Natural move: reuse the backend
   venv, which already carries most of those deps (and would retire
   `build_sandbox_snapshot.py`'s duplicate manifest).
3. **Platform assumptions.** Prompts instruct the model that `python3` exists and
   `python` doesn't — inverted on Windows. `als`/`aglob` shell out to GNU
   `find -printf`, which BSD/macOS `find` lacks. A local backend on Windows/macOS
   needs those revisited; the remote sandbox hid all of it.

### Recommended shape (if approved)

A **factory, not a replacement** — keep both paths behind
`MINIME_EXECUTION_BACKEND=local|langsmith`:

```
backend/execution.py  ->  LazyLangsmithSandbox(thread_id)                    # default, unchanged
                      ->  LocalWorkspaceBackend(LocalShellBackend)           # root_dir=<app_data>/threads/<id>,
                                                                             # virtual_mode=True, env={ASTA_TOKEN}
```

Wire it at `agent.py:86` + `routes/common.py:41` and **touch nothing else** —
`mcp_tools.py`, `theory_tools.py`, `datavoyager_tools.py`, `middleware/sync.py`,
`routes/rendering.py` are already duck-typed against the surface. Then revisit
`prompts.py` (path + `python3` rules) and `guardrails.py` (the isolation
assumption).

**Verdict: medium-low code risk, medium-high behavioural risk.** The plumbing is
a small bounded diff; the isolation question is the actual decision. Keeping the
remote sandbox behind a flag makes it reversible and lets the desktop app opt in.

**On isolation, decided:** for a local-first app the user runs on their own
machine against their own files, host execution is the *point*, not a regression —
the same trade Zed, Claude Code, and every local dev tool make. But
`guardrails.py` currently *states* it relies on sandbox isolation, so that
assumption must be rewritten rather than silently invalidated, and the
**human-gated** policy is honoured by putting approval on the `execute` tool
(deepagents recommends HITL here). Local ≠ ungoverned.

---

## 11. Experiment: what actually breaks without LangSmith / WorkOS (2026-07-30)

Rather than argue about the dependency surface, we measured it. A **stripped
overlay** of the backend was assembled in scratch space — every directory
symlinked to the real checkout (which stayed untouched, `git status` clean) plus a
hand-written `.env` containing **only** `OPENAI_API_KEY`, `ASTA_API_KEY`,
`ASTA_TOKEN`, and `LANGSMITH_TRACING=false`. No `LANGSMITH_API_KEY`, no
`WORKOS_*`, and those names were scrubbed from the launching environment too. Then
`--check-backend --stream` ran a real turn against it.

**Result:**

| Layer | Without LangSmith + WorkOS |
|---|---|
| Server boot / graph import | ✅ works |
| `GET /ok` health | ✅ works |
| Auth (`POST /threads`) | ✅ works — unauthenticated `local-user`, thread created |
| Tracing | ✅ fine, silently off |
| **Agent run** | ❌ **fails** |

The failure is precise and singular:

```
SandboxSyncMiddleware.before_agent
  -> sandbox.aget_work_dir()  (backend/sandbox.py:259)
  -> aresolve()               (backend/sandbox.py:161)
  -> langsmith client.get_sandbox(...)
  -> SandboxAuthenticationError: 401 Unauthorized
     https://api.smith.langchain.com/v2/sandboxes/boxes/minime-<thread-id>
```

**Conclusions.**

1. **WorkOS is already droppable — zero code change.** Local mode never
   authenticates; `auth.py` admits `local-user`. `vault.py` (WorkOS Vault for
   storing user keys) simply goes unused when keys come from the environment or
   the OS keychain.
2. **LangSmith *tracing* is droppable — one flag.** `LANGSMITH_API_KEY` appears
   **nowhere** in backend code; it is purely SDK-implicit.
3. **The LangSmith *sandbox* is the single hard blocker**, and it fails *before
   the agent even starts* (in `before_agent` middleware) — so nothing works
   partially. Replace the execution backend (§10) and LangSmith drops out
   entirely.
4. **Two externals remain by nature:** the **Asta** API/CLI and the **model API**.
   Those are the product, not infrastructure. The honest privacy claim is
   therefore *"no infrastructure services, and your files never leave your
   machine"* — not "no network".

This is the whole justification for §10, measured rather than assumed.

---

## 12. P6.3 step 1: the composer (2026-07-30)

Until now the app could only send one hardcoded prompt — the gap between a demo
and a tool. It now has a real text field: type, press **Enter**, the turn streams.

**Why this was the expensive step.** GPUI ships **no text-input widget** — only
primitives (focus, key actions, IME plumbing, `shape_line`). Its own
`examples/input.rs` is **746 lines**, because an input means cursor motion,
selection, clipboard, grapheme-aware boundaries, IME pre-edit, *and* a custom
`Element` that lays out the line and paints the caret. We adapted that example
into `crates/app/src/composer.rs` rather than hand-rolling a lesser one (decision
taken 2026-07-30). It is Apache-2.0, same as `gpui`; attribution is in `NOTICE`.

**Changes from upstream:**

- **Enter submits**, emitting `ComposerEvent::Submit(text)`; the parent view
  decides that means "run a coordinator turn". Empty/whitespace input is ignored.
- **Cross-platform bindings** — `ctrl-a/c/v/x` as well as `cmd-`; the example is
  mac-only and our primary dev machine is Windows. Bindings are scoped to a
  `Composer` key context so `enter` doesn't leak into other surfaces.
- **A disabled state** — the field is read-only while a turn is in flight.
- **No let-chains** — the example uses them; they need edition 2024, we're on 2021.
- Dark-theme placeholder, accent-coloured caret.

**Also in this step:**

- **The transcript scrolls** (`id` + `overflow_y_scroll`) — previously long
  conversations just ran off the bottom.
- **Empty assistant turns are dropped.** A failed run used to leave a blank
  `you`/`mini-me` pair in the transcript (visible in the P6.2 Windows screenshot).

**Known limitation:** single-line by design. `shape_line` lays out one line, so
soft wrap and `shift-enter` for a newline need a different layout path — deferred.

**Verified:** builds clean, clippy clean, and a real turn still streams
end to end headlessly (102 chunks). Typing itself needs a human at a window.

---

## 13. Windows is the target — what that costs P6.2.5 (2026-07-30)

~98% of our users run Windows. Linux is where we develop; Windows is where the
product lives. This reorders the local-execution work (§10) rather than the UI.

**The problem.** `LocalShellBackend.execute` runs `subprocess.run(..., shell=True)`,
which on Windows is **`cmd.exe`**. Mini-Me's tool layer builds **POSIX** command
strings — `cmd >/dev/null 2>&1; cat /tmp/…`, `… | python3 -c <reducer>` — and its
prompts instruct the model that `python3` exists and `python` does not. On Windows
all of that is wrong. The remote sandbox has been hiding it, because the sandbox is
Linux no matter what the client runs.

So "move execution to the host" is **not** platform-neutral: done naively it works
on our dev Linux box and fails for essentially every real user.

**The options, honestly:**

| Option | Cost | Consequence |
|---|---|---|
| **WSL2 runs the backend** (app stays native Windows, talks to `127.0.0.1:2024`) | An extra install step per user; WSL2 must be enabled | Real Linux userspace, so `bash`/`python3`/`asta` all behave; keeps the local-first story intact. Localhost forwarding makes the client unchanged. |
| **Keep the remote sandbox on Windows** | None | No install pain, but LangSmith stays, and the "no infrastructure, files never leave your machine" claim dies for 98% of users. |
| **Make the tool layer shell-agnostic** (upstream) | Largest change: rewrite the POSIX command construction in `theory_tools.py`, `datavoyager_tools.py`, prompts | The only option that makes native Windows a first-class execution host. Best long-term, most work, and it is upstream code. |

Note one thing that got *easier*: `LocalShellBackend`'s `ls`/`glob` are pure Python
(`rglob`) and `grep` falls back to Python when the binary is absent — so the GNU
`find -printf` shims in `sandbox.py` do **not** need porting. The shell is the
remaining problem, not file operations.

**Decided 2026-07-30: WSL2.** Confirmed available on the target machine
(`wsl --status` → default distro Ubuntu, version 2).

**Why WSL2 won.** The decisive argument isn't that it dodges `cmd.exe` — it's that
**inside WSL the backend simply *is* on Linux**, so the §10 local-execution design
works exactly as written, with **zero upstream changes to Mini-Me's tool layer**.
That matters because "bundle upstream, never fork" is a locked decision: WSL2
shrinks P6.2.5 from "rewrite the tool layer's shell handling" to "swap one backend
class". The client↔backend boundary is HTTP on localhost — which WSL2 forwards — so
the backend's OS is genuinely an implementation detail. Two bonuses: `uv sync` of
the PyMC/scikit-learn stack is far more reliable on Linux than through MSVC on
Windows (a support burden avoided), and it's the same environment we develop in.

**Why not the others.** *Remote sandbox* was ruled out as the primary path because
it needs a **LangSmith API key per user** — we cannot ship ours, so every scientist
would register an account on a free tier allowing one concurrent sandbox; worse
onboarding than WSL2 *and* it contradicts the privacy premise. It stays as a
documented fallback for machines where WSL2 is blocked by IT. *Shell-agnostic tool
layer* remains the best end state but is large upstream work that still can't fully
succeed, since the **model** writes shell commands at runtime — closing that would
mean constraining execution to Python-only. Worth revisiting later, incrementally.

**Implemented (client side).** `MINIME_BACKEND_WSL=1` (or a distro name) launches
the sidecar via `wsl.exe [-d <distro>] -- bash -lc "cd <dir> && exec
.venv/bin/langgraph dev --host 0.0.0.0 …"`, with `MINIME_BACKEND_WSL_DIR`
(default `~/Mini-Me`) giving the checkout path *inside* the distro. Details that
matter:

- **`--host 0.0.0.0`**, not loopback: WSL2's localhost forwarding reliably reaches
  services bound to all interfaces; loopback-only binds are not always visible.
- **`exec`** so the login shell is *replaced* by the server — otherwise killing our
  child leaves the real process running.
- **Teardown also runs `pkill -f "langgraph dev"` inside the distro**, because
  killing `wsl.exe` does not reliably reap the Linux process it fronted.
- The repo-layout check is skipped in WSL mode (we can't cheaply stat the distro's
  filesystem from Windows), and `current_dir` is *not* set — pointing `wsl.exe` at
  a host path is meaningless and would fail the spawn if it didn't exist.

`scripts/setup-wsl.sh` provisions the distro (uv, clone, `uv sync --extra dev`,
`.env` template); it is idempotent and never overwrites an existing checkout or
`.env`.

**Accepted wrinkle:** Windows files reach the backend as `/mnt/c/...`, so the
"drop a CSV, no upload dance" flow needs host→WSL path translation. That is the
P6.4 *local file → analysis* seam, ~10 lines, but it must be designed rather than
discovered.

**Still to verify on Windows (cannot be tested from the Linux dev box):** that
`wsl.exe` spawning works end to end, that localhost forwarding reaches the server,
and that teardown leaves no process behind.

---

## 14. Async subagents (P6.5) — and the sidecar-lifetime question they force

Evaluated 2026-07-30 against LangChain's
[async subagents](https://docs.langchain.com/oss/python/deepagents/async-subagents)
and [interpreters](https://docs.langchain.com/oss/python/deepagents/interpreters)
docs plus the live Mini-Me code. **Verdict: adopt async subagents as P6.5; skip
interpreters.**

### Where we actually are today (the premise, corrected)

The stack is *not* uniformly synchronous. The two genuinely long jobs already
don't block a chat turn — they submit and return, and the **client** polls:

- `hypothesis_generator` (theorizer) and `data_voyager` (DataVoyager) run
  `asta … --no-wait`, return `task_id` + `status="running"`, park the id in graph
  state, and the frontend polls `/theorizer/{thread}/{task}` and
  `/analyze-data/{thread}/{task}` until terminal. DataVoyager's own docstring:
  *"20–40 min for multi-step modelling, so — exactly like the theorizer — Mini-Me
  does NOT block a chat turn on it."*

So the worst case was hand-solved with bespoke plumbing. What **does** block is the
other eight subagents — `data_cleaning`, `exploratory_data_analysis`,
`diagnostic_analytics`, `predictive_analytics`, `report_writer`,
`academic_researcher`, `dataverse_explorer`, `pdf_librarian`. Seconds to minutes
each, with the conversation frozen throughout.

### Why async subagents fit *this* product

1. **The conversation stays live.** Launch an EDA and keep working — refine the
   mission, chase literature — while it runs. That is what "acceleration" means
   for a workbench.
2. **One mechanism instead of bespoke polling.** `start_async_task`,
   `check_async_task`, `update_async_task`, `cancel_async_task`,
   `list_async_tasks`, with task metadata in a dedicated `async_tasks` state
   channel that survives summarization. Long term this could retire the custom
   poll routes and per-tool poll code.
3. **`list_async_tasks` is the data model for a Jobs panel.** Background jobs +
   tray notifications + "close the window and come back" is precisely the native
   affordance we justified this app with (P6.4). This is the feature that makes
   desktop worth the rewrite.

### The four real costs

1. **Preview API.** `deepagents 0.6.1` does export `AsyncSubAgent` /
   `AsyncSubAgentMiddleware` (verified in the installed package), but the docs
   flag it preview: *"APIs may change."* Same class of churn risk we escaped with
   gpui — acceptable only with a pinned version.
2. **Each async subagent must be its own graph** (`graph_id` on an Agent Protocol
   server). Mini-Me declares **one** graph today (`agent` in `langgraph.json`), so
   this is a structural upstream change — in the repo we deliberately do not fork.
   Co-deployed ASGI mode (omit `url`) keeps it in-process with no network hop,
   which is the right starting point for a local sidecar.
3. **Worker starvation — measured, and it applied to us.** `langgraph dev`
   defaults to **one** concurrent job
   (`langgraph_api/cli.py`: `n_jobs_per_worker if … is not None else 1`). Async
   subagent runs are separate runs on separate threads, so with a single slot the
   supervisor's own run holds it and the child run queues — the feature would look
   broken. **Fixed now:** the sidecar launches with `--n-jobs-per-worker 10` on
   both the host and WSL paths. This already pays off for concurrent turns across
   threads/windows, independent of async subagents.
4. **It contradicts our sidecar lifetime.** ⚠️ **The open design question.** The
   supervisor kills the backend when the window closes, so background jobs would
   die with it. "Run in the background" and "the backend is a child of the window"
   are incompatible. Options: let the sidecar outlive the window (detached, with
   adoption on next launch — note the app already health-checks and attaches to a
   running backend, so the machinery half-exists); or keep the current lifetime and
   rely on jobs being resumable by `task_id` after a restart. **Decide before
   building P6.5.** Either way it needs a **Jobs panel** with visible state and
   cancel, so background work stays observably human-gated.

Documented model-discipline failure modes to expect, all mitigated only by prompt
engineering (upstream, fragile): supervisors polling immediately after launch
(turning async back into blocking), truncating `task_id`s, and reporting stale
status instead of re-checking.

### Why not interpreters

Not a rival to async subagents, and a poor fit for us on two counts:

1. **It bypasses the human gate.** *"PTC calls do not go through the normal tool
   calling path. As a result, `interrupt_on` approval workflows are not enforced
   per PTC-invoked tool call."* Our policy is human-gated; a mechanism that fans
   out tool calls around the approval path is a policy problem, not a feature gap.
2. **Wrong runtime for our workload.** QuickJS — JavaScript, 5s default timeout,
   64 MB heap, no filesystem/network. Our compute is pandas/PyMC/scikit-learn in a
   sandbox. The docs themselves scope interpreters to in-memory orchestration and
   point at sandboxes for real execution.

The legitimate use — collapsing multi-step *orchestration* (dedupe/merge/score
across many tool results) into one turn — is real but minor next to (1).

### Sequencing

**P6.5, after P6.2.5.** Building it before the panels and local execution would
stack two unsettled foundations. Prerequisites: pin the deepagents version, answer
the sidecar-lifetime question, and design the Jobs panel.

---

## 15. Agent activity: streaming subagent work and steps (2026-07-30)

**The gap.** The app renders only the coordinator's final text. Ask *"find the
deseq2 paper"* and you get a long silence, then an answer — while underneath, a
subagent ran a literature search. The web frontend surfaces this; we don't, and the
logic lives in TypeScript, so it has to be ported.

Measured on a real turn (`find the deseq2 paper`, `stream_subgraphs=true`,
718 KB captured), which is what makes this designable rather than guesswork:

| event | count | what it is |
|---|---|---|
| `messages\|tools:<uuid>` | **319** | the subagent's own token stream |
| `messages` | 176 | coordinator tokens + tool-call chunks |
| `updates` | 35 | node-level state changes |
| `updates\|tools:<uuid>` | 8 | subagent node changes |
| `values`, `values\|tools:…` | 6, 5 | state snapshots (already consumed) |
| `custom` | 2 | `sandbox_status` (already consumed) |

**Attribution is clean.** The `messages` tuple's metadata names the subagent
outright:

```json
{ "lc_agent_name": "academic_researcher",
  "checkpoint_ns": "tools:d6c187d3-…",
  "langgraph_node": "model",
  "ls_model_name": "gpt-5.4" }
```

So: namespace `tools:<uuid>` identifies *an invocation*, `lc_agent_name` gives the
*display name*, and `checkpoint_ns` groups a subagent's events together. The
LangChain docs describe the same thing as the `ns` tuple with
`any(s.startswith("tools:"))`.

**Steps are derivable from tool-call chunks.** Coordinator `messages` chunks carry
`tool_call_chunks`; on this turn: `task` (the deepagents delegation tool) and
`search_paper_by_title`. That yields real step labels — *"delegating to
academic_researcher"*, *"searching papers"* — without inventing anything.

**Two honest findings that shape the design:**

1. **There is no "thinking" channel today.** Every content block on that turn was a
   plain string — no `thinking`/`reasoning` block types. The event-streaming docs
   don't cover reasoning either. So what we can show is *work and steps*, not
   chain-of-thought. If a reasoning-exposing model is configured later, non-text
   content blocks would carry it and the same decoder path can surface it.
2. **Raw `updates` is unusable as UI.** Of the 35 events, almost all are middleware
   plumbing: `PIIMiddleware[email].before_model`, `ModelCallLimitMiddleware.*`,
   `TodoListMiddleware.after_model`, `SkillsMiddleware.before_agent`… Only `model`
   and `tools` are meaningful. The docs make the same point, recommending a filter
   to "interesting nodes". We should not render this stream directly.

### Design (P6.3 step 6)

1. Request `stream_subgraphs: true` — currently off, which is *why* none of the 319
   events reach us. Our SSE decoder already matches the `messages|<ns>` prefix, so
   the transport work is small.
2. Extend `TurnEvent` with `SubagentToken { agent, text }` and `Step { label }`,
   keyed off `lc_agent_name` / `checkpoint_ns` and `tool_call_chunks`.
3. Render an **activity trace** attached to the in-flight assistant turn:
   one collapsible group per subagent (`▸ academic_researcher`), streaming its text
   live, auto-collapsing when the turn completes so the transcript stays readable.
   Steps appear as one-line entries.
4. Keep the coordinator's answer visually primary — the trace is context, not the
   deliverable.
5. Do **not** render `updates`; if step granularity is later wanted beyond tool
   calls, filter to `model`/`tools` explicitly.

**Cost note:** subagent tokens outnumber coordinator tokens roughly 2:1 on a simple
literature lookup. The trace must be cheap to render and collapsible, or a long
research turn will bury the answer.

A full captured stream is kept in the session scratchpad as a decoder fixture, so
this can be built and unit-tested without burning tokens on live runs.

### 15b. How the web frontend does it (read-only audit, 2026-07-31)

Audited the React app to port rather than reinvent. **Four findings change §15's
design.**

**1. The logic is in the SDK, not the app.** `filterSubagentMessages` is not a local
helper — it is an option on `useStream` from `@langchain/react`, implemented inside
`@langchain/langgraph-sdk`. The app supplies ~40 lines of glue
(`ThreadStreamSession.tsx:54-74`); the SDK does namespace parsing, tool-call
correlation and chunk accumulation. **Porting means reimplementing SDK behaviour**,
which is a bigger job than §15 first implied.

**2. The web app *displays* subagent work — we are catching up, not leading.**
Subagent messages are stripped from the main transcript and re-routed into a
per-subagent side channel rendered as live collapsible cards:
`SubagentActivityPanel` (left sidebar) → `SubagentCard` (spinner, status pill, live
subtitle, tool list, markdown result). Chat also gets a one-line
`describeActivity` summary ("Academic Researcher · <task>", "Coordinating N
subagents…").

**3. Attribution: the SDK's path is fragile — ours can be simpler.**
- The SDK attributes `messages` events via `metadata.langgraph_checkpoint_ns`
  (fallback `checkpoint_ns`), splitting on `|` and taking the first `tools:` segment.
  Other modes (`updates`/`values`/`custom`) are attributed by the **event-name
  suffix** instead. Two different paths.
- **The namespace id is a pregel task UUID, *not* the `tool_call_id`.** The SDK
  reconciles them by matching the subgraph's first `HumanMessage` content against
  the `task` tool call's `description` argument — a three-pass heuristic (exact →
  substring → pending-retry) that can mis-attribute when two subagents receive
  identical descriptions in one turn.
- **Our measured shortcut:** the `messages` metadata already carries
  **`lc_agent_name: "academic_researcher"`** (§15). For *displaying* named,
  grouped subagent activity we can key off `lc_agent_name` + `checkpoint_ns` and
  **skip the description-matching heuristic entirely**. We only need the harder
  correlation if we want to tie a card to its originating `task` tool call (for the
  task description and the terminal `ToolMessage`). Prefer the simple path first.

**4. Reasoning is not rendered anywhere, and the extractor silently drops it.**
`messages.ts:37-58` duck-types on the presence of a `text` field with **no `type`
discrimination**, so an Anthropic-style `{type:"thinking", thinking:"…"}` block
yields `""` and disappears. "Thinking…" in the UI is a hardcoded placeholder. The
app never requests `events` mode either. Combined with §15's measurement (all
content blocks were plain strings under `gpt-5.4`), the honest position stands:
**no reasoning is available today**, and if a reasoning-exposing model is
configured, *not* dropping non-text blocks is a place we can exceed the web app.

**Other details worth copying:**

- Effective stream request: `stream_mode` = `messages-tuple`, `values`, `updates`,
  `custom`; `stream_subgraphs: true`; `config.recursionLimit: 10000`. (We were the
  only client running on LangGraph's default limit of 25 — **fixed 2026-07-31**.)
- Subagent registration accepts a tool call only when `name == "task"` **and**
  `args.subagent_type` matches `^[a-zA-Z][a-zA-Z0-9_-]{2,49}$` — a guard against
  half-streamed JSON args. Stored args are upgraded only when the new value is
  *longer*.
- Lifecycle: `pending` (registered from tool call) → `running` (first namespaced
  `updates`) → `complete`/`error` (main-namespace `ToolMessage` matched by
  `tool_call_id`).
- Tool calls pair with results by id; state is `error` if the result errored,
  `completed` if a result exists **or any later AI message exists** (an
  approximation worth deciding on deliberately rather than copying), else `pending`.
- Main transcript filter (`shouldRenderMainMessage`): user/assistant only, non-empty
  text, and excluding `message.name ∈ {academic_researcher, dataverse_explorer,
  data_cleaning, exploratory_data_analysis, diagnostic_analytics,
  predictive_analytics, report_writer}`. **Consequence:** a delegation turn that is
  *purely* tool calls renders as nothing in chat — its visibility comes entirely
  from the subagent panel. That is exactly the silent gap we see today.
- Truncation budgets: result preview 50 000 chars, tool result 480, tool args 200.
- **Theorizer and DataVoyager progress cards are HTTP-polled, not streamed** —
  `GET /theorizer/{thread}/{task}` and `GET /analyze-data/{thread}/{task}` every
  30 s while the artifact is `running`. A stream-only client will never show their
  progress; that needs a polling loop (own milestone, not part of §15).
- Also stream-fed: a todo/plan progress bar from `values.todos`, and the sandbox
  pill from `custom` (we already consume the latter).

*Caveat: this audit read the SDK's compiled `dist/` JavaScript, so names are
minifier-influenced though the logic is intact.*

### 15c. What was built (P6.3 step 6, shipped 2026-07-31)

**The gap is closed.** Ask *"find the deseq2 paper"* and the transcript now reads:

```
mini-me
· delegating to academic_researcher — Find the canonical DESeq2 paper. Return a concise citation…
▾ academic_researcher · 2 steps · 722 chars
    · search_papers_by_relevance
    · get_paper
    The canonical DESeq2 paper is the 2014 Genome Biology article… · 1 sources
Love MI, Huber W, Anders S. 2014. Moderated estimation of fold change …
```

Verified against a live delegating turn (`--check-backend --prompt "find the deseq2
paper"`, 2026-07-31): the delegation, both subagent tool calls and 722 characters of
subagent text all arrived. Note the tools differ from the §15 capture
(`search_papers_by_relevance` + `get_paper` vs `search_paper_by_title`) — the trace
reports what the run actually did, it does not replay a script.

**Where it lives.** `protocol::TurnDecoder` (the decoder), `TurnEvent::Step` /
`TurnEvent::SubagentToken` (the wire→UI contract), `AgentTrace` in `main.rs` (the
per-invocation group), `Workbench::activity_block` (the render).

**Six decisions worth keeping straight:**

1. **Attribution is by SSE event name, not metadata.** A subagent's frames arrive as
   `messages|tools:<uuid>`; the coordinator's arrive as plain `messages`. The
   metadata's own `langgraph_checkpoint_ns` is *not* usable as the discriminator —
   measured, top-level frames carry `model:<uuid>` there, which names a node, not a
   delegation. Display name comes from `lc_agent_name`, so §15b's
   description-matching heuristic was never needed.
2. **The grouping key is the whole namespace.** The JS SDK keys on the *first*
   `tools:` segment; we keep `tools:a|tools:b` intact, so a nested delegation gets
   its own group under its own name instead of being filed under its parent's while
   wearing the inner agent's label.
3. **The decoder had to become stateful.** Only the *first* `tool_call_chunk` of a
   call carries its name and id; later fragments are keyed by `index` alone. The
   `task` delegation's label lives in arguments that arrived across **60 fragments**,
   so `TurnDecoder` accumulates them and announces once — using "does the JSON parse
   yet" as the completeness signal, since the backend leaves `chunk_position` null.
   `subagent_type` is shape-checked (`^[a-zA-Z][a-zA-Z0-9_-]{2,49}$`, as the web
   client does) so a half-streamed value can't become a label.
4. **A subagent's "text" is often not prose — this was the surprise.** On the
   measured turn `academic_researcher` streamed its entire answer as *one JSON
   object* (its structured response, 678 chars over 173 frames). Dumping it would
   show the user a wall of braces, so `summarize_agent_result` lifts `summary` out
   and counts the array fields (`… · 1 sources`). A partial object still streaming,
   or genuine prose, passes through untouched — which incidentally makes the trace
   look alive and then resolve into a sentence.
5. **`values|tools:…` is deliberately ignored.** The subagent's own snapshot carries
   the same artifacts as the coordinator's, three events earlier; consuming both
   would render the outputs twice. `updates` is still not requested at all.
6. **Activity counts as content.** `finish_turn` used to drop an assistant message
   with an empty body. A purely delegated turn *has* an empty body (§15b), so that
   would have thrown away the only record of the work — the condition is now
   `is_silent()`: no text **and** no steps **and** no traces.

**Cost control.** Each group caps its stored text at 4 000 characters, dropping from
the *front* — a trace is a tail-followed log. New groups open expanded (they are what
is happening now) and all collapse when the turn ends, so the answer stays primary.

**Two new verification paths, both free:**

- `--replay <capture>` decodes a saved SSE capture and prints the transcript it would
  produce. No backend, no window, no tokens. The full 718 KB capture lives outside
  the repo at `~/Documents/mini-me-desktop-fixtures/subagent-stream-sample.txt`.
- `crates/app/tests/fixtures/delegated-turn.sse` (50 KB) is that capture reduced to
  fit the repo — middleware `updates` dropped, metadata narrowed to the fields a
  client reads, single-token text frames coalesced, tool results truncated, but
  **every `tool_call_chunks` fragment verbatim**, because that is where the only
  stateful logic lives. It replays to byte-identical output and is asserted by
  `a_real_delegated_turn_produces_one_named_trace_with_its_steps`.
- `--check-backend --prompt "…"` runs any prompt headlessly and prints steps and a
  per-subagent tally, so the trace can be checked on the Linux box where no window
  can open.

**Still not available, and not faked:** no reasoning/thinking channel (every content
block on the measured turns was a plain string), and **no per-subagent completion
signal** — the terminal `task` `ToolMessage` arrives in the *main* namespace and
can't be tied to a namespace without §15b's heuristic, so groups simply collapse when
the turn ends rather than showing a false "done" tick. Theorizer/DataVoyager progress
is still HTTP-polled upstream and remains unimplemented here.

## 16. Rendering markdown — and the visual-layer decision it forces (2026-07-31)

**The gap.** The coordinator writes markdown and we render the source. A real answer
currently reads `Love MI et al. 2014. **Moderated estimation…**` with the asterisks
showing. This is **not cosmetic**: reports, citations and tables *are* the
deliverable of this product, and a citation the user has to mentally de-escape is a
worse artifact than the web app's.

Measured on the DESeq2 turn, the coordinator emitted `**bold**`, `*italic*`, a bare
URL and a hard line break — so the minimum useful set is inline emphasis, inline
code, links, headings, lists, code blocks and tables (report subagents emit tables).

**What GPUI gives us.** `gpui 0.2.2` has the primitives but no markdown element:
`StyledText::with_highlights(Vec<(Range<usize>, HighlightStyle)>)` for inline runs,
and `InteractiveText` for clickable ranges (links). Zed's own markdown crate is not
something we can depend on — it is wired into Zed's internal `ui`/`theme`/`language`
crates. So the block layer (paragraphs, lists, tables, code fences) is ours to write
as GPUI elements either way.

**Two ways to close it, and a genuinely surprising option B:**

| | A — our own renderer | B — adopt `gpui-component` |
|---|---|---|
| Dependency | `markdown = "1.0"` (CommonMark → AST), 1 crate | `gpui-component 0.5.1`, **58k LOC**, 31 required deps (tree-sitter, html5ever, ropey, rust-i18n, lsp-types) |
| Effort | ~250 lines: walk the AST, emit divs + highlight runs | wire in its `TextView` |
| What we get | exactly the subset we need | markdown *plus* tables, theming, `dock` panel layout, notifications, virtual lists, spinners, a full text input |
| What we give up | tables and code highlighting cost extra work | our hand-rolled composer and palette become redundant; we inherit its theme system and its release cadence |

**The surprise:** `gpui-component 0.5.1` (Apache-2.0, Longbridge) depends on
**`gpui = "0.2.2"` — the exact version we pinned**, from crates.io, not a Zed git rev.
So there is no two-incompatible-gpui problem, which is normally what rules these
libraries out. It is a real option, not a fantasy.

**Recommendation: A now, B as a deliberate decision later.** A is proportionate to
the gap, keeps rebuild time low (which the "click to update" story in P6.4 depends on
— every update is a rebuild on the user's machine), and leaves B open. B is worth its
weight only if we decide to buy the *whole* visual layer at once, and that is a
locked-decision-level call for the visuals milestone, not something to slide in
under a markdown ticket.

**Sequencing.** Not part of P6.3. This is the first item of the visuals pass, before
the palette gets prettier and before any theming work — because every other visual
improvement is judged against text that is still showing its asterisks.

## 17. P6.3 step 5: the command palette — and the thread bug it exposed (2026-07-31)

`ctrl-p` / `cmd-p` opens a ranked, filterable command list; `↑↓` moves, `⏎` runs,
`esc` closes, and the status bar carries a `ctrl-p commands` hint because a palette
nobody knows the shortcut for is a palette nobody opens.

**Commands:** Run turn · New thread · Refresh project spine · Expand agent activity ·
Collapse agent activity · Copy last answer · Quit. A closed enum, not a registry of
closures: every command is reachable another way too, so there is nothing dynamic to
register.

**The bug it exposed.** Adding "New thread" made no sense until we looked — because
`run_turn` called `POST /threads` **on every turn**. Each question was its own
conversation, so a follow-up started from nothing. One `Arc<Mutex<Option<String>>>`
in `Sidecar` fixes it: create on first use, reuse after, and `reset_thread()` is what
"New thread" now means. Nothing is deleted server-side; we just stop adding to the
old thread, and the spine is thread-independent so the mission survives.

*Verified live:* `--check-backend --prompt "find the deseq2 paper" --prompt "who is
the first author of that paper?"` → turn 2 answered **"Michael I. Love."** in 5
chunks with no subagent and no re-search, on the same thread id. That answer was
impossible before the fix.

**Three implementation notes worth keeping:**

1. **Ranking, not filtering.** A plain subsequence test is too loose for a palette:
   `nt` also matches "ru**n** **t**urn" and "expa**n**d ac**t**ivity". So matches are
   *scored* — 8 for a word-initial hit, 1 mid-word, +4 for adjacency — and sorted, so
   `nt` puts "New thread" under the cursor without hiding the rest. Declaration order
   breaks ties, which keeps an empty query in a stable, authored order.
2. **The query field is a second `Composer`.** Reusing it gives the palette real text
   editing (selection, clipboard, IME) for nothing. It needed one new flag,
   `submits_empty`: in the chat composer an empty Enter is nothing to send, but in the
   palette Enter means "run the highlighted command" and must fire before anything is
   typed. It is created once and kept, so its subscriptions register once rather than
   per-open.
3. **Focus has to be handed back explicitly.** An entity subscription has no `Window`,
   so activating with Enter can't refocus the composer directly — a `restore_focus`
   flag is settled in `render`, which does have one. Without it, focus would sit on a
   field that is no longer rendered and typing would go nowhere.

**The headless check now runs the real path.** `check()` used to call `stream_turn`
directly with its own thread; it now goes through `run_turn` — the same function the
window uses — so it covers thread reuse rather than just the HTTP surface, and
repeating `--prompt` is how multi-turn continuity gets verified with no window.

## 18. P6.2.5: host execution, shipped as an overlay (2026-07-31)

**Acceptance met.** A real turn ran with **no `LANGSMITH_API_KEY` and no `WORKOS_*`**,
executing on this machine:

```
$ MINIME_BACKEND_DIR=<checkout with those keys stripped> MINIME_EXECUTION_BACKEND=local \
  mini-me-desktop-app --check-backend --prompt "compute the mean of [2,4,6,8] with pandas,
  write it to result.txt…"
status   : Local workspace: …/local-workspaces/019fb99b-…
step     : ls
step     : execute
--- assistant text ---
5.0
$ cat …/019fb99b-…/result.txt
5.0
```

`asta` resolves on `PATH` (0.101.1) with `ASTA_TOKEN` reaching executed commands, so
theory generation, DataVoyager and PDF extraction have what they need.

### The answer to "where does the change land"

**Neither a PR in Mini-Me nor a fork: a `PYTHONPATH` overlay in this repo.**
`overlay/` ships a Python package that the app injects; the checkout stays byte-for-byte
upstream. That is what the locked decision — *"bundled upstream, pinned, unmodified —
not forked"* — actually asks for, and it means a `git pull` in Mini-Me can never
conflict with us.

Mechanism: `PYTHONPATH=overlay/` makes Python auto-import `overlay/sitecustomize.py`,
which registers an import hook; when the backend later imports `backend.sandbox`, the
hook rebinds `LazyLangsmithSandbox`. Both construction sites (`backend/agent.py`,
`backend/routes/common.py`) import that name at *their* module load, so one rebinding
covers both — the "~3 lines" §10 identified, achieved with zero edits.

An import hook rather than a startup `import backend.sandbox`: for a console script
`sys.path[0]` is `.venv/bin`, and it is LangGraph that puts the checkout on the path
later while resolving `langgraph.json`. Hooking the import removes that ordering
guesswork. **The upstream seam is still the nicer destination** — if Mini-Me grows a
real `MINIME_EXECUTION_BACKEND` factory, `workspace.py` moves across almost verbatim
and the hook disappears. This is the bridge, not a rejection of the PR.

### Five things the audit and the live run corrected

1. **The replacement is thinner than §10 thought, for a different reason.** Every `a*`
   method in deepagents' `BackendProtocol` is a *concrete default* that offloads its
   sync twin with `asyncio.to_thread`. So subclassing `LocalShellBackend` inherits the
   whole async surface Mini-Me awaits — nothing to write. (`BaseSandbox`, which
   upstream's sandbox extends, needs only 4 abstract methods and implements file ops
   *in terms of* `execute`; interesting, but a dead end for us.)
2. **`virtual_mode=False`, not `True` as §10 recommended.** Upstream's tools build
   absolute paths from `aget_work_dir()` — `f"{work_dir}/theories/{task_id}"` — pass
   them to the file operations *and* print them in tool output the model then opens
   with executed Python. Both sides must agree on one namespace. Virtual mode re-roots
   only the file operations (deepagents is explicit that it never constrains
   `execute`), so `/workspace/x` would mean two different things. Verified: `awrite`
   then `cat` sees the same file.
3. **Absolute *writes* still get re-rooted.** With `virtual_mode=False` deepagents
   passes absolute paths through, but upstream's `_resolve_for_write` sent anything
   outside the work dir to `<work_dir>/<basename>`. We mirror it, and locally it does
   double duty as the only guardrail the file tools have: `write("/etc/hosts", …)`
   lands harmlessly in the workspace. Reads need nothing — `virtual_mode=False`
   already means "absolute as-is, relative under cwd", which is what upstream's
   `_resolve_for_read` arranged.
4. **`langgraph dev` runs a blocking-call detector, and it failed the first turn.** A
   bare `Path.mkdir` inside an `async def` raised `BlockingError: Blocking call to
   os.mkdir` and aborted the run in `SandboxSyncMiddleware.before_agent`. Every
   filesystem touch in the overlay now goes through `asyncio.to_thread`. Only a live
   run finds this.
5. **The overlay must not follow commands into their child processes.** `PYTHONPATH` is
   inherited, so every command the model ran re-imported `sitecustomize` and its
   startup line landed in the command's stderr — which `execute` merges into the
   output the *model* reads. The command environment now has the overlay stripped out.

Two smaller things worth keeping: `python3` is pointed at the venv interpreter via
`sys.executable`'s directory (the app launches `.venv/bin/langgraph` without
activating, so `python3` would otherwise be a bare system interpreter with no pandas —
this also retires `build_sandbox_snapshot.py`'s duplicate manifest), and truncation is
imported from upstream rather than reimplemented, so the cap behaves identically
(measured: 32 KB cap, 50 KB survives the untruncated path).

### Still open — and why it is not on by default

**Host execution is opt-in; the sandbox remains the default.** Turn it on with
`--local` (or `MINIME_EXECUTION_BACKEND=local`); `--sandbox` forces it back off. The
flags win over the variable on purpose: PowerShell has no `VAR=value cmd` prefix form,
and a `$env:` assignment persists for the whole session — which has already produced one
confusing debugging session on this project. The app logs a warning when host execution
is on, and the status bar says `host (local)` in the accent colour.

Flipping the default is gated on **human-approval for `execute`**. Org policy is
human-gated and deepagents explicitly recommends HITL for this backend; the file tools
have the re-rooting guardrail but `execute` has nothing, and locally that means the
user's own files. That needs `interrupt_on` in the agent plus an approve/reject path in
the Rust client — a new streaming concern (interrupt + resume), so it is its own step
rather than a rider on this one.

Also still true: **`guardrails.py` upstream still states the design relies on sandbox
isolation.** We cannot rewrite it from an overlay, and it should not be silently
invalidated — which is another reason the default stays where it is. And the sandbox
snapshot pins `asta v0.101.0` while this host has `0.101.1`; a startup version check
is still owed.

## 19. Host execution becomes the default, gated by approval (2026-07-31)

**Decided:** local is the default; nothing runs in the remote sandbox unless someone
asks for it with `--sandbox` (or `MINIME_EXECUTION_BACKEND=sandbox`).

What made that safe to do is the other half of this step: **every `execute` call now
stops and asks.** The run pauses, the desktop shows the command verbatim, and nothing
happens until the researcher approves or rejects.

```
step     : execute
approve  : execute — python3 - <<'PY' ⏎ value = 7 * 6 ⏎ … ⏎ PY
--- assistant text ---
42
$ cat …/answer.txt → 42
```

Verified with **no flags set** — default path, no `LANGSMITH_API_KEY`, command held,
approved, run resumed on the same thread.

### The gate

`create_deep_agent(interrupt_on={"execute": {"allowed_decisions": ["approve","reject"]}})`,
plus the same key on every subagent dict — most execution happens *inside* subagents
(data cleaning, EDA, predictive modelling), so gating only the coordinator would leave
the majority of commands unreviewed. Upstream already uses this mechanism
(`diagnostic_analytics` interrupts on `request_diagnostic_context`), so the shape is
Mini-Me's own, not an invention.

`edit` and `respond` are not offered: the client can approve or reject, and advertising a
decision the UI cannot produce would strand the run.

### Two things the measurements changed

1. **The import hook has to target `deepagents`, not `backend.agent`.** LangGraph loads
   the graph module by *file path* (`langgraph.json` → `./backend/agent.py:agent`) via
   `spec_from_file_location`, which bypasses `sys.meta_path` — so a hook on
   `backend.agent` never fires in the real server. This failed silently and looked like
   a working gate: the sandbox patch landed, the approval patch did not, and the command
   ran. `backend/agent.py` does `from deepagents import create_deep_agent`, and *that*
   goes through normal machinery, so patching the package attribute first is what takes
   effect. **A patch that can fail quietly is worse than one that cannot.**
2. **We were already broken on interrupts, before any of this.** Upstream's
   `diagnostic_analytics` gate means a paused run has always been possible — and to our
   client a pause is indistinguishable from a finished stream: the SSE connection simply
   ends. So that subagent's turns died silently with no answer. `TurnOutcome` now
   distinguishes them, and `Sidecar::resume` continues the turn on the same thread.

`__interrupt__` arrives inside the `values` frame we already request, so no new stream
mode was needed. Payload:
`[{"value":{"action_requests":[{"name","args","description"}],"review_configs":[{"action_name","allowed_decisions"}]},"id"}]`,
and the resume body is `{"command":{"resume":{"decisions":[{"type":"approve"}]}}}` — one
decision per held action, in order, which the middleware validates.

### Also added

`MINIME_CAPTURE_SSE=<path>` appends the raw stream to a file. Every wire shape in this
plan was measured that way; now it is a flag instead of a one-off probe, and what it
writes is exactly what `--replay` reads back.

### What this leaves open

- **`guardrails.py` upstream still says the design relies on sandbox isolation.** That
  sentence is now wrong for the default path. It cannot be fixed from an overlay, and it
  is the strongest remaining argument for eventually sending Mini-Me a PR.
- **Approval fatigue is untested.** A long analysis may hold a dozen commands. If it
  becomes tiring in practice the answer is remembered decisions (per-command-shape
  allowlists), not removing the gate — `MINIME_APPROVE_EXECUTE=0` exists but is not a
  recommendation.
- **The gate has never been driven by a human.** Approve/Reject buttons are unverified
  in a window; the headless check auto-approves because it cannot ask anyone.
- `asta` version pinning (sandbox pinned `v0.101.0`, host has `0.101.1`) is still owed.

## 20. Settings panel and secrets (added 2026-07-31, on request)

**This is a prerequisite for the installable, not a nicety.** The whole "click an icon"
goal dies if the first thing a researcher must do is hand-edit a `.env` file inside a WSL
distro. The settings panel is what replaces that.

### What actually has to be collected

Audited from `.env.example` and every `os.getenv` in the backend (read-only, 2026-07-31).
Dropping WorkOS and LangSmith removes most of it:

| | key | why |
|---|---|---|
| **required** | one model provider key — `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GOOGLE_API_KEY`, `MISTRAL_API_KEY`, or a custom OpenAI-compatible base URL | nothing runs without one (`backend/models.py`) |
| **required for research tools** | `ASTA_TOKEN`, `ASTA_API_KEY` | literature search, theorizer, DataVoyager, PDF extraction |
| **no longer needed** | `LANGSMITH_API_KEY`, `WORKOS_CLIENT_ID`, `WORKOS_API_KEY`, `AUTH_ALLOWED_EMAIL_DOMAINS`, `MINIME_SANDBOX_SNAPSHOT` | §11 and §19 removed the need |

Plus the desktop's own, which are *settings, not secrets*: model choice
(`MINIME_DEFAULT_MODEL`), backend port, checkout location, WSL on/off and distro,
execution locality, approval on/off, workspace root.

### The split, and why it matters

**Two stores, deliberately.** Settings go in a plain `settings.toml` under the platform
config dir — readable, diffable, safe to paste into a bug report. Secrets go in the **OS
keychain** (Windows Credential Manager / Secret Service / macOS Keychain) via the
`keyring` crate. A key must never land in a file the user might sync, zip, or attach; and
CIP policy is that credentials stay the user's own, on the user's own machine.

### Providers: upstream already built this for a panel

`backend/models.py`'s table is commented *"provider id (from the panel)"* — the web app
has a model-config panel and the backend takes its keys **per request**, so the desktop
should speak the same contract rather than invent one:

```json
"config": { "configurable": {
  "model_config": {
    "default": "anthropic::claude-sonnet-4-5",
    "subagents": { "data_cleaning": "openai::gpt-4o-mini" },
    "storage_mode": "client"
  },
  "__llm_keys": { "anthropic": { "api_key": "…", "base_url": null } },
  "__is_for_execution__": true
} }
```

Providers: `openai`, `anthropic`, `google`, `mistral`, and **`custom`** — an
OpenAI-compatible endpoint with a mandatory `base_url`, which is how OpenRouter, Groq,
Ollama, vLLM and friends are reached. Model specs are `"provider::model_id"`.

Three consequences that improve the design above:

1. **Model keys never have to become environment variables at all.** They go from the OS
   keychain into the request body, in memory — not into `.env`, not into the sidecar's
   environment, not onto a `wsl.exe` command line. That is a *better* security property
   than the env-var plumbing, and it removes the `WSLENV` concern for these keys entirely.
   `ASTA_TOKEN`/`ASTA_API_KEY` still need the environment, because the `asta` CLI reads
   them when `execute` runs a command — so `WSLENV` applies to those two only.
2. **`storage_mode: "client"` sidesteps the server-side Vault.** Left unset with no inline
   keys, the backend tries a Vault lookup that needs a user identity — i.e. the WorkOS
   world we dropped. Saying "client" explicitly keeps that path dormant.
3. **Per-subagent model overrides are free.** `model_config.subagents` already routes each
   subagent to its own model, so "cheap model for data cleaning, strong model for theory"
   is a panel row, not a feature to build. Worth exposing once the basics work.

Switching provider or model then needs **no sidecar restart** — it is just the next
request's config.

### Getting the Asta credentials into the sidecar without a `.env`

The launcher already injects environment variables (§18/§19), so the same seam carries
the two Asta credentials — which is what makes the checkout's `.env` *optional* and the
install clickable. One wrinkle worth getting right: **secrets must not go on the
`wsl.exe` command line**, where `ps` would show them. WSL's documented mechanism is
`WSLENV` — set the variables on the `wsl.exe` process and list their names in `WSLENV`,
and the distro inherits them. That is the plumbing to use, not the `VAR=… exec …` prefix
the execution flags use.

### Panel design (Zed-shaped)

- `ctrl-,` opens **Settings** as a pane, plus a palette entry. Not a modal dialog — Zed's
  lesson is that settings you can leave open while you work get fixed.
- Sections: **Model** (provider — including a custom OpenAI-compatible endpoint with its
  base URL — plus key and model id) · **Research tools** (Asta) ·
  **Execution** (host/sandbox, approval on/off, workspace root) · **Backend** (checkout,
  port, WSL distro).
- Secret fields are **masked**, show only "set / not set" once stored, and are never
  logged. This needs one new `Composer` capability — a mask mode — which is a small
  addition to what §12 already built.
- A **Test** button per section: for the model key, start the sidecar and run the trivial
  seed turn; for Asta, `asta --version` through the backend. Better a failure the user
  sees here, next to the field, than a cryptic error on their first real question.
- **First-run**: with no model key stored, the app opens Settings instead of letting a
  turn fail against a backend that cannot answer. That is the "setup tutorial" item made
  concrete — a filled-in panel beats a document.

### Interaction with the native-Windows question

Only the delivery detail depends on it: on WSL the keys travel via `WSLENV`, natively they
are plain child-process variables. The panel's UI and both stores are the same either way,
so this is *not* blocked on that probe.

## 21. Native-Windows probe: verdict (2026-07-31)

**Question:** now that host execution is the default, can we drop WSL2 and make the
installer "unzip and run"?

**Answer: not on `cmd.exe`, and not by re-prompting the model — because the POSIX
assumptions are in Mini-Me's *own tool code*, not in what the model writes.**

Evidence (read-only audit):

```python
# backend/theory_tools.py:246-247  — the theorizer
fetch = f"asta generate-theories task {shlex.quote(task_id)} 2>/dev/null"
return f"{fetch} | python3 -c {shlex.quote(_REDUCE_TASK_PY)}"

# backend/datavoyager_tools.py:_export_shell  — DataVoyager artifact export
f"mkdir -p {run_dir} && "
f"asta analyze-data task {tid} > {run_dir}/task.json 2>/dev/null && "
f"asta artifacts --input {run_dir} --output {export_dir} --format md 2>/dev/null"
```

`2>/dev/null`, `|`, `&&`, `mkdir -p`, and **`shlex.quote`** — which emits POSIX
single-quoting that `cmd.exe` passes through literally, quotes included. These are the
two headline research features, so "it mostly works" is not an option.

**What *did* clear up since §13:** the GNU `find -printf` dependency is gone (our
backend's file operations come from deepagents' pure-Python `FilesystemBackend`, not
upstream's shell-based sandbox), and `python3` resolves to the venv interpreter because
the overlay sets `PATH` (§18). So the remaining blocker is narrower than it was — it is
now *only* the shell dialect.

### Three options, and the recommendation

| | viability |
|---|---|
| **`cmd.exe` natively** | ❌ ruled out by the evidence above |
| **WSL2** | ✅ works today, verified end to end (§13, §18, §19) |
| **Native + a POSIX shell** (Git Bash / MSYS2) | 🟡 plausible, untested |

The third is genuinely interesting and cheap to try, because our overlay already owns
`aexecute`: route commands through `bash -lc` instead of letting `subprocess.run(shell=True)`
pick `COMSPEC`, add a `python3` shim next to the venv's `python.exe`, and Git for Windows
is a small silent-installable dependency compared with enabling a Windows feature and
provisioning a distro. The open risk is MSYS path translation — our `aget_work_dir()`
returns `C:\Users\…`, and drive-letter paths inside MSYS bash need testing on real
Windows, which cannot be done from the Linux dev box.

**Recommendation: keep WSL2 as the supported runtime for v1**, and put the effort into
making *provisioning* automatic (`wsl --install`, then `scripts/setup-wsl.sh`) rather than
betting the packaging design on an untested shell. Native-plus-bash stays a documented
follow-up that could simplify the installer later; it is a half-day experiment on a
Windows machine, not a prerequisite.

**Consequence for P6.4b:** the installer's job is a guided first run — detect WSL, offer
`wsl --install`, provision the distro, then hand off to the settings panel (§20). Nothing
about that is blocked by this verdict, which is why it was worth spending a day to get it
before designing the installer rather than after.

## 22. P6.4a part one: settings store, keychain, and the key path (2026-07-31)

**Proven:** a turn ran against a checkout whose `.env` contained **no provider key at
all**. The key came from the OS keychain, the model choice from `settings.toml`, and both
travelled in the run request. That is the mechanism a clickable install needs — nobody has
to edit a file inside a WSL distro to get started.

```
$ mini-me-desktop-app --set-secret llm:openai "sk-…"
llm:openai: stored in the OS keychain
$ mini-me-desktop-app --check-backend --prompt "…"      # .env has no OPENAI_API_KEY
--- assistant text ---
settings path works
```

### What exists now

- **`settings.rs`** — `Settings` (provider, model id, base URL, host execution, approval,
  port) in `settings.toml` under the platform config dir, plus keychain access. Every
  field defaults, so a file from an older build still loads.
- **Two stores, as designed (§20):** settings in plain TOML; keys in the OS keychain.
- **The request contract** — `model_config.default` = `"provider::model_id"`,
  `__llm_keys.<provider> = {api_key, base_url}`, `storage_mode: "client"`,
  `__is_for_execution__: true`. Also sent on **resume**, so a continuation cannot silently
  lose the key mid-turn.
- **`--set-secret NAME [VALUE]`** writes one credential and exits, never echoing the
  value. An empty value forgets it. The panel is the real interface; this is how a headless
  machine gets set up, and it is what made the test above possible.
- **Asta credentials** reach the backend as environment variables (they must — the `asta`
  CLI reads them when `execute` runs), via `WSLENV` in WSL mode rather than the command
  line.
- Settings now drive the port, execution locality and the approval gate, with environment
  variables still winning as the debugging escape hatch.

### Three things worth keeping

1. **`storage_mode` is omitted when there is no key.** Claiming client-only storage with
   nothing to supply would tell the backend to skip its own lookup and then find nothing —
   a confusing failure instead of a working fallback to its environment.
2. **Keychain reads must not happen on a Tokio thread.** The Linux client (zbus) runs its
   own `block_on`, so reading a secret from inside the runtime panics with *"Cannot start a
   runtime from within a runtime"* — which is exactly how the first live run died. Secrets
   are now read once on the main thread, before any runtime exists, and passed in.
3. **No `libdbus-1-dev`.** `keyring`'s default Linux backend needs that plus `pkg-config`;
   the zbus backend (`async-secret-service` + `crypto-rust`) is pure Rust. `cargo build` on
   a fresh machine has to just work.

### Not done yet — the panel itself

This is the plumbing, not the UI. Still to build: the `ctrl-,` Settings pane, masked secret
fields (needs a mask mode on `Composer`), the per-section **Test** button, and the
first-run behaviour of opening Settings instead of letting a turn fail. Until then the CLI
is the only way to store a key, which is fine for us and not fine for a researcher.

**Unverified:** keychain read/write has only been exercised on Linux/zbus. Windows
Credential Manager is the path that actually matters and needs a run on Windows.

### 22b. The Settings pane (2026-07-31)

`ctrl-,` (or the palette's **Settings**) opens Settings in place of the artifacts panel —
a pane, not a modal, so it can be left open while you work.

- **Provider** cycles on click through Anthropic / OpenAI / Google / Mistral / Custom.
  Five options do not need a dropdown, and a dropdown is a widget GPUI has none of.
  Switching also suggests a model that exists for the provider just chosen, rather than
  leaving one that does not.
- **Base URL** only appears for the custom provider, which is the only one that requires it.
- **Secret fields open empty and are masked.** What is in the keychain is never read back
  into the UI; the row says `· stored` or `· not set` instead. Leaving a field blank on
  save keeps what is already there — so changing your model does not mean re-pasting your
  key. Saving clears the field.
- **Toggles** for host execution and the approval gate.
- **Problems are listed before you hit them** — a custom provider with no base URL, a
  missing key — using the same `Settings::problems` the startup log uses.
- **First run opens the pane** with "Add a model key to get started", instead of letting a
  turn fail against a backend with no key.

**Masking is byte-for-byte.** The composer replaces each *byte* of the content with `*`, so
the mask is exactly as long as the text. Cursor and selection are byte offsets into the
string being shaped, and a mask of a different length would put the caret in the wrong
place or panic on a character boundary. Keys are ASCII in practice, so the count is exact.

**What applies when.** The model and key take effect on the **next turn** — the backend
resolves them per request, so `Sidecar::set_model` swaps them behind a lock with no
restart. The port and execution locality are baked into the sidecar's launch command, so
those need a restart, and the pane says so rather than leaving the user to wonder.

**Verified on Windows 2026-07-31:** the pane renders, keys store to **Windows Credential
Manager**, and a turn runs using a key read from there. So the whole point of §20 — a
researcher configuring the app without touching a `.env` inside WSL — works on the target
platform.

**Verified on Windows 2026-07-31 (second pass):** the approve/reject buttons on a held
command, the activity trace's delegation view, and the palette with arrow-key navigation
all work.

**Spanish keyboard verified 2026-07-31:** `¿qué papa es mas resisñente?` typed and
submitted intact — dead-key accents, `ñ` and inverted punctuation all survive the
composer's grapheme handling. That was the last open verification item for P6.3/P6.4a.

### The bug that pass found

**Suggestions vanished when the answer arrived**, so they could not be clicked. Cause: our
client treated every spine payload as authoritative, but upstream recomputes suggestions
opportunistically — `ProjectSpineMiddleware.abefore_agent` derives them from whatever
artifacts the thread has and emits a payload carrying mission and completed work even when
it produces none. Measured: every `values` snapshot in a turn had `suggestions: 0`.

Fixed by distinguishing advisory content from state: a payload without suggestions means
"no new advice", not "the advice is withdrawn", so suggestions survive while mission /
completed / pending still replace. Clicking one now also removes it from the list, since it
is in the composer at that point. **Only a human watching would have caught this** — every
headless check passed throughout, which is worth remembering the next time a panel looks
fine in a test.

*Also noted:* closing the window logs `window not found` and two invalid-window-handle
HRESULTs from GPUI's Windows text-input teardown, after the sidecar has already stopped.
Cosmetic shutdown noise, not a crash — a polish item.

## 23. Markdown rendering (2026-07-31)

The asterisks are gone. `**bold**`, `*italic*`, `` `code` ``, `[text](url)`, `#` headings,
`-`/`1.` lists, fenced code and `---` rules now render; anything else is shown as typed.

**Hand-written, not a parser crate** (option A of §16). GPUI has no Markdown element, so the
block layer had to be built regardless; the inline layer is then a few hundred lines against
a *measured* subset, with no dependency to track. Inline styling uses
`StyledText::with_highlights` — one shaped line per block with ranges carrying the
differences — which is how GPUI wants it, rather than a tree of nested elements.

Four decisions worth keeping:

1. **The user's own text is never reinterpreted.** Only assistant messages go through the
   parser: rewriting someone's asterisks in their own prompt would be presumptuous.
2. **A link keeps its URL beside the text.** Nothing is clickable yet, and dropping the URL
   would lose the DOI — the part of a citation a researcher actually needs.
3. **`snake_case` does not become italics.** `read_file` and `write_file` in one sentence
   would otherwise italicise everything between them, which is a real thing this
   coordinator writes about.
4. **Half-written markup renders as typed.** Text streams in token by token, so the
   transcript is *constantly* showing unclosed markers; an unterminated `**` must not
   swallow the rest of the line or make it disappear.

**The bug the tests caught:** stepping one *byte* past a marker lands inside `á`, and slicing
there panics. Every other branch is safe because it only steps past an ASCII marker, but the
plain-text branch had to advance by a whole character. Spanish text would have crashed the
renderer on the first accented word — worth remembering that ~98% of users type Spanish.

**Not covered:** blockquotes, nested lists, and images. (Tables were on this list and now
render — §27.) Code has no monospace face — no font is bundled — so
it is marked by colour instead, which is honest but not ideal.

**Unverified:** never rendered. Everything here rests on unit tests over measured output.

**Verified on Windows 2026-07-31** — bold, italics, inline code and links all render; tables
are still literal, and the user's own Spanish (`¿qué papa es mas resisñente?`) came through
intact, which is the accented-character path the boundary fix was for. Tables deferred by
agreement rather than by omission.

## 24. P6.4b part one: the Setup pane (2026-07-31)

**The problem.** A machine that was not already provisioned produced
`backend did not become healthy within 120 attempts` in the status bar. That is true, and
it is useless — the real answer is always one of a short list: WSL is not installed, the
checkout is not there, `uv sync --extra dev` was never run, the overlay is unreachable, or
no model key is stored. §21 settled P6.4b's shape as "a guided first run"; this is the part
that does the guiding.

`preflight.rs` asks those questions and returns each answer **with the command that fixes
it**. `ctrl-p → Setup & diagnostics` opens it, `--preflight` prints the same thing
headlessly and exits non-zero, and a turn that fails to *start* now opens the pane instead
of naming a log file (`looks_like_a_setup_failure`, whose marker strings are pinned by a
test because the routing reads them).

### Four things that make the checks trustworthy

1. **Every probe runs where the backend runs.** `BackendConfig::shell_argv` routes through
   `wsl.exe -- bash -lc` or plain `bash -lc`, the same hop as the launch command. Checking
   for `langgraph` on the Windows side would report green for a machine that cannot launch
   anything — a check on the wrong side of that boundary is worse than no check.
2. **WSL is probed by asking the distro to answer**, not by parsing `wsl -l`: that command
   prints **UTF-16LE**, which `from_utf8_lossy` turns into NUL-riddled nonsense. Round
   -tripping `echo ok` through bash also proves a distro is *usable* rather than merely
   registered. For the same reason `wsl.exe`'s own stderr is never displayed.
3. **Nothing can hang.** `Command::output()` has no timeout and a half-installed WSL can
   block rather than fail; probes poll `try_wait` and kill the child at 30s. A setup pane
   that spins forever is worse than the message it replaced.
4. **Failures never cascade.** No runtime means the checks that run *inside* it report
   `Skip`, with the reason naming what they actually wait on.

### The check that exists because the failure is silent

Host execution works by putting `overlay/` on the backend's `PYTHONPATH` so
`sitecustomize` swaps the sandbox class at interpreter startup (§18). If that path is not
reachable from the backend — the repo on a drive the distro has not mounted, a UNC path
`wsl_path` cannot translate — **Python imports nothing and raises nothing**, and the
backend quietly tries the *remote* sandbox instead. The user then sees an authentication
error about a service they thought they had stopped using. Nothing else in the app would
have caught that, so `overlay` is its own row.

### Two real defects found while building it

- **`cd '~/Mini-Me'` does not work.** Quoting suppresses tilde expansion, so bash looks for
  a directory literally named `~`. The launch command had been quoting nothing at all, so a
  configured `MINIME_BACKEND_WSL_DIR` containing a space would have split into a bogus
  command. `quote_path` now quotes only what follows the tilde: `~/'My Repos/Mini-Me'`
  expands *and* survives the space, which `Documents\My Repos\…` makes a real case.
- **A skip that named the wrong dependency.** On a machine with a working shell and no
  checkout, the dependencies row said "the runtime above has to work first" — sending the
  user to check WSL when the checkout was the problem. Caught by running the failure paths
  rather than by a test, then pinned with one.

### Deliberately not done yet

**No "Run" button on the fixes.** These commands clone repositories and ask for admin
rights; firing one from a click with no visible output would be the app's least accountable
moment. Each fix is shown as a copyable command instead. Streaming the runner — with live
output in the pane — is part two, and it is what turns "click to update" from a plan into a
button.

**Verified:** `--preflight` on the Linux dev box reports 5 ok / 1 to fix (no Anthropic key
stored here), and the three failure paths were run by hand — an empty checkout dir, WSL mode
where `wsl.exe` does not exist (which is exactly what Windows-without-WSL looks like), and
`--sandbox`. 59 tests pass. **The pane itself has never been rendered.**

## 25. P6.4b part two: install it for them (2026-08-01)

*"Do all the necessary so it works without complications. Remember that our users don't
know how to code anything."* — that instruction settled several questions that had been
open, and invalidated one thing the plan had assumed.

### The thing that was wrong: a private repo cannot be cloned by a scientist

Provisioning ran `git clone` against
`CENTRO-INTERNACIONAL-DE-LA-PAPA/Mini-Me`, which is **private**. GitHub stopped
accepting account passwords for git in 2021, so what that prompt actually wants is a
**personal access token**. No amount of UI polish makes "create a PAT" a reasonable first
step, and with `stdin` closed — which it must be, or the app hangs on an invisible prompt
— the clone simply fails.

So **the backend travels with the app.** `scripts/bundle-backend.sh` puts a pinned,
unmodified checkout in `vendor/` (gitignored — the locked decision is *bundled, never
forked*, and a vendored copy in git is a fork with extra steps), and
`BackendConfig::setup_script` passes it as `MINIME_BUNDLED_SOURCE`. Whoever prepares a
build needs GitHub access once; nobody else ever does. This also gives "click to update"
its real shape: the backend is version-matched to the app, so updating the app updates the
backend, and no user-side credentials are involved either way.

### Where the checkout lives, and who owns it

`~/.local/share/mini-me-desktop/backend`, **inside the distro** — not in the desktop
repo, and not on `/mnt/c`. WSL2 reaches Windows drives over a 9p mount whose per-file cost
is high, and a Python environment holding the scientific stack is thousands of small files
stat'd on every interpreter start. A venv there is the one placement guaranteed to feel
broken.

More important is **ownership**, now recorded in `settings.toml` and on `BackendConfig`:

| | the app may update it? |
|---|---|
| **Owned** — the app provisioned it | ✅ yes |
| **Adopted** — discovered, or set via `MINIME_BACKEND_*_DIR` | ❌ never |

This is not fussiness. Updating means `git checkout <pin>` + `uv sync`, and the reference
checkout on this developer's own machine has **ten local branches, several live in
worktrees**. Pointing an update button at a directory the app did not create is how you
destroy someone's work. The pane says which case applies, in words, because it changes
what the app is allowed to do to the user's files.

When a checkout *is* discovered, the pane offers **"Use the one I have"** before "Install
Mini-Me" — adopting takes a second and preserves their branches; installing a second copy
costs gigabytes.

### Fixes now run, with their output on screen

`preflight::run_streaming` + `Sidecar::run_fix` spawn the command and stream it line by
line into the pane. Three decisions:

- **Streamed, not buffered.** Provisioning takes minutes. A spinner with no detail is
  exactly the experience this pane exists to replace.
- **stdout and stderr on separate threads.** Reading them in sequence deadlocks the moment
  a chatty child fills the pipe nobody is draining — and `uv` writes its progress to
  stderr, which is most of what there is to watch.
- **`stdin` is null**, so nothing can wait on an invisible prompt, and ANSI codes are
  stripped because GPUI renders escape sequences as the mojibake they are.

A successful fix **re-checks by itself**, so the row the user just fixed turns green
without them having to work out that "Re-check" was the next step.

### The overlay stops depending on the Windows drive

Provisioning copies `overlay/` to `<checkout>/.desktop-overlay`, and the launch command
prefers that copy — decided by the distro's own shell at launch:

```
PYTHONPATH="$(if [ -f ~/'…/.desktop-overlay'/sitecustomize.py; then … else … fi)"
```

Not by probing from Windows: a `wsl.exe` round trip costs seconds on every start, and
there is nowhere to cache the answer that would not go stale the moment the user
re-provisioned. This retires the silent failure §24 built a check for.

### Also fixed

- **The `.env` template is gone.** It told users to paste keys into a file inside a Linux
  distro — which §22 made unnecessary and this instruction makes unacceptable. The script
  writes an intentionally empty `.env` (because `langgraph dev` auto-loads one, and its
  absence made people think they had missed a step) whose entire content explains that
  keys live in the app.
- **Setup is the front door.** Preflight runs on every launch, and the *first* report
  opens Setup when something blocks a turn — outranking the old "no key → Settings",
  because pasting a key into an app that cannot start its backend fixes nothing. Later
  re-checks never steal the pane; the user has seen the state of things by then.
- **A real bug in the script:** `${BASH_SOURCE[0]}` was resolved *after* `cd "$DIR"`, so a
  relative invocation looked for the overlay in the wrong place and silently skipped
  installing it. Found by running the script, not by reading it.

### Verified

The whole loop, on the Linux dev box, with `HOME` and `MINIME_DATA_DIR` redirected to
simulate a fresh machine:

1. `--preflight` → `3 ok · 2 to fix · 1 skipped`, "not installed", with the bundled source
   threaded into the install command.
2. That exact command run → copies from the bundle, discards the source machine's venv,
   installs the overlay, syncs `--extra dev`, confirms `langgraph` exists, exits 0.
3. `--preflight` again → **`5 ok · 1 to fix`**, the overlay now resolving to the
   provisioned copy. The only thing left is the model key, which is a Settings click.

Plus: the `PYTHONPATH` expression exercised in real bash, both branches, with a space in
the path (tilde expansion and quoting interact badly and it had to be checked, not
assumed); and the provisioned overlay confirmed importable. 61 tests pass.

**Not verified:** none of the pane has been rendered, and no Windows path has run — no
WSL on this box. The `wsl.exe --install` fix in particular is written from documentation,
not from a machine.

### Still open

- **Cancelling a running fix.** With `stdin` closed the realistic stalls are network ones,
  and the output makes a stall visible, but there is no button to stop it.
- **A prebuilt binary.** Users still `cargo build`, and `overlay_dir()`/`scripts_dir()` are
  compiled-in paths that assume a checkout. `MINIME_OVERLAY_DIR`/`MINIME_SCRIPTS_DIR`
  already exist for a packaged layout; nothing has been packaged.
- **Windows Job Object teardown** (§9) — still the last item on P6.4b.

### 25b. What rendering it on Windows found (2026-08-01)

The Setup pane ran on Windows and the guided install worked: **5 ok · 1 optional**, the
backend provisioned into `~/.local/share/mini-me-desktop/backend` inside the distro, with
the streamed output ending in "Done — Mini-Me is ready." Three things the screenshot
exposed, none of which a test would have.

**The pane named a different overlay than the launch would use.** It reported
`/mnt/c/Users/…/mini-me-desktop/overlay` while the launch command was already preferring
the copy provisioning had installed inside the distro. A check that reports a different
path from the one in use is worse than no check — it sends anyone debugging to the wrong
file. Both now come from `BackendConfig::overlay_candidates()`, one definition, so they
cannot drift again.

**The Asta CLI is a button now.** `allenai/asta-plugins` is **public** (Apache 2.0) —
unlike Mini-Me — so unlike the backend it needs no credentials and really can be installed
in one click: `uv tool install git+…@v0.101.1 && uv tool update-shell`. Pinned to the
version the Asta plugin itself pins (`skills/asta-cli/SKILL.md`), with the tag verified
against the remote and the install actually run end to end (it produced a working
`asta 0.101.1`). Bump both together — a CLI newer than the skills driving it is how a
subcommand goes missing.

**A PATH hazard that was working by luck.** The app launches the backend with `bash -lc`
— a login shell that is *not* interactive — which reads `~/.profile` and **never**
`~/.bashrc`, because Ubuntu's `.bashrc` returns in its first few lines when `$-` has no
`i`. The setup script had been writing its `~/.local/bin` PATH line only to `.bashrc`,
where the backend could never see it. It worked anyway because Ubuntu's default `.profile`
adds that directory itself. That is luck, and `asta` is precisely the tool that has to be
found on the backend's PATH at execute time, so the script now guarantees it (guarded, so
a distro that already handles it is left alone).

## 26. Shipping it: process teardown and a folder you can send (2026-08-01)

### The Job Object

Windows has no process group to signal, and killing a parent leaves its children running.
`uv run` forks the real server as a grandchild, and `wsl.exe` fronts a process living in
another kernel — both survived `Child::kill`, kept holding port 2024, and made the next
launch attach to a stale backend.

A **Job Object** with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is the OS-level answer: every
process in the job dies when the last handle closes. Crucially that covers the app
**crashing** — the handle closes with the process, so the kernel cleans up even when no
destructor of ours runs. `taskkill /T` would only work during an orderly shutdown.

**Verified by cross-compiling**, which is worth recording as a technique: the whole crate
cannot be checked for `x86_64-pc-windows-msvc` from Linux (gpui pulls `stacker`, whose
build script needs `windows.h`), but the `job` module can be extracted into a throwaway
crate with only `windows-sys` and checked there. That found two missing feature gates —
`CreateJobObjectW` is behind `Win32_Security`, `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`
behind `Win32_System_Threading` — which would otherwise have been a broken Windows build
discovered by the user. The extraction is scripted against the real file, so what was
checked is what ships.

Known gap: there is a window between spawn and `AssignProcessToJobObject` in which a
grandchild could escape. Closing it needs `CREATE_SUSPENDED`, which
`std::process::Command` does not expose. The child spends its first moments importing
Python, so the race is theoretical.

### A folder, not an installer

`scripts/package.sh` assembles `dist/mini-me-desktop/`: the executable beside `overlay/`,
`scripts/` and `vendor/Mini-Me`. **Deliberately not an MSI** — no code signing, no
notarization. The audience is a few dozen colleagues, and an unsigned installer is a
SmartScreen warning that teaches people to click through security dialogs.

What made this work is `resource()`: look at an env override, then **beside the
executable**, then the compiled-in repo path. Before that, `CARGO_MANIFEST_DIR` was baked
in at build time, so a shipped copy would have hunted for the overlay under a path that
existed only on the build machine — and, because a missing overlay fails *silently*
(§24), quietly fallen back to the remote sandbox.

The script refuses to be quiet about the one thing that would break a colleague's install:
if `vendor/Mini-Me` is absent it says so loudly, because without it the user is asked for
a GitHub token they do not have.

**Verified:** the bundle was copied to an unrelated directory with a fresh `HOME`, and
`--preflight` resolved the overlay, the setup script and the bundled backend **entirely
inside the bundle**, with no path pointing at the source tree. (First attempt reported the
source tree — a stale binary, because `cargo test` had run but `cargo build` had not. That
trap has now cost this project twice.)

## 27. Two debts that would have grown (2026-08-01)

### Tables

Report subagents emit them, and they were rendering as literal pipes — the one Markdown
gap that touched the actual deliverable.

**Recognised by the separator row (`|---|`), never by pipes alone.** This coordinator
writes about shell pipelines and alternatives constantly (`asta search | head -5`,
`main | develop`), and a parser that treated every pipe as a cell boundary would shred
ordinary sentences into columns. That means one line of lookahead, which is the only
structural change the parser needed.

Four decisions:

- **Ragged rows keep every cell they have.** Text streams token by token, so half-written
  tables are on screen constantly; a short row must not make the block vanish, and a long
  one must not be truncated. Column count comes from the widest row.
- **Escaped pipes stay inside their cell** — the agent writes regular expressions and
  shell commands into tables.
- **Cells are still Markdown**, because a bold verdict in a results table is the norm.
- **Equal-width columns.** GPUI has no table layout, and measuring text before shaping is
  not something this app can do honestly. Even columns are predictable; a naive
  proportional split collapses a column to nothing when one cell is long.

Outer pipes are optional, since models emit both GitHub styles.

### Approval fatigue

Every `execute` stopped and asked. In a real analysis that is ten identical dialogs, and
the tenth is not read — it is dismissed. Then neither is the eleventh, which is the one
that mattered. A gate that trains people to click through it is worse than no gate,
because it also carries the appearance of review.

The card now offers **"Approve the rest of this turn"**. Deliberately *not* a persistent
"always allow": that converts one bounded decision into a permanent one, and a stale
allowlist is invisible. One task's remaining commands is a decision someone can hold in
their head, and it expires by itself — `finish_turn` clears it, next to where the pending
request is cleared, so the two cannot drift.

Approved commands still appear in the activity trace. This removes the *interruption*, not
the record.

### A flaky suite, found by running it twice

The packaging test creates a directory beside the test binary and sets `MINIME_OVERLAY_DIR`;
the ownership test redirects `HOME`. `cargo test` runs tests as **threads in one process**,
so those writes changed what every concurrently running test saw. The suite passed with
`--test-threads=1` and failed at random otherwise — worse than a failing test, because it
teaches people to re-run until green.

Fixed with a shared `env_lock` that every environment-touching test takes first, rather
than pinning the whole suite to one thread. Poisoning is recovered from: the guarded data
is `()`, so one panicking test must not cascade into every other. Confirmed by running the
suite five times.

### Deliberately still open

**The multi-line composer.** The field is single-line at the layout level, so this is not a
key binding — it is soft wrap, cursor movement across visual lines, and a growing input
height. Half-implemented text editing is worse than none, and pasted newlines are still
flattened to spaces (`composer.rs`), which is a real if minor loss when someone pastes a
multi-paragraph question. Sized as its own piece of work rather than squeezed in here.

## 28. It ships, and it does the thing the web app can't (2026-08-01)

### Verified on Windows, end to end

`bundle-backend.sh` → `cargo build --release` → `package.sh` → a **21 MB** folder, and the
packaged binary ran a real coordinator turn with the spine populating beside it. That is
P6.4b's core proven on the target platform, not inferred from Linux.

Two corrections to what this plan assumed:

- **Size: 21 MB, not the 1–2 GB estimated.** That figure came from the *debug* binary
  (718 MB); release strips it to 18 MB, and the backend source is 3.5 MB. The bulk lands
  on the user's machine at install time, when `uv sync` builds the environment — which is
  the right place for it, since those wheels are machine-specific anyway.
- **Release builds need `fxc.exe`.** gpui pre-compiles its HLSL shaders only when
  `debug_assertions` is off (`build.rs:259`), so `cargo build` works for months and the
  build that actually ships fails. Its search is `GPUI_FXC_PATH`, then `PATH`, then **one
  hardcoded SDK version** — a different Windows SDK is enough to fail. Build-time only:
  the bytecode is `include!`d into the binary, so nobody receiving the zip needs an SDK.

Two papercuts fixed along the way, both the same shape — *a prompt that cannot be
satisfied*. `git clone --reference <local> <url>` still contacts the remote for refs, so
the bundle asked for a GitHub password despite existing to avoid one; and redirecting
stderr does **not** suppress git's credential prompt, which is written straight to the
terminal, so the update path asked again. `GIT_TERMINAL_PROMPT=0` is the fix for the
second. Git inside WSL has no credential helper at all, which is a third variant, now
documented.

### Local file → analysis

The MVP bar was "one thing the web app can't do" (§5). This is it: **drop a file on the
window** and the question is prepared for you. No upload, no bucket, no copy — the
researcher's data is already on this machine, and that is the entire advantage of being
native.

Three decisions:

- **The path is translated to the backend's view.** On Windows the agent lives inside WSL,
  where `C:\Users\…\yield.csv` is `/mnt/c/Users/…/yield.csv`. A prompt naming the Windows
  path would send it looking for a file that does not exist there, and the researcher
  would have no idea why. `path_for_backend` does this once, and a test asserts no
  backslash survives.
- **Referenced, never copied.** Keeping a scientist's data where they put it is most of
  the point; a copy in a working directory goes stale the moment they edit the original.
- **Loaded into the composer, not sent.** Dropping a file is a clumsy gesture that happens
  by accident, and this is the same rule the suggestion cards already follow — the app
  prepares the question, the person asks it.

Dropping is accepted anywhere on the window rather than on a designated strip: someone
dragging a file has their eyes on the file.

**Unverified:** never dropped anything. `on_drop` is wired to the root and the translation
is tested, but no file has been dragged onto a real window.

## 29. P6.5, redirected: collect the long jobs that were already running (2026-08-01)

§14 planned P6.5 as **deepagents async subagents**. Reading the code before building it
found a blocker and, more importantly, a live defect that mattered more.

### The blocker

Async subagents require **each async subagent to be its own graph** on the Agent Protocol
server. Mini-Me declares exactly one (`agent` in `langgraph.json`), so this is a structural
change to a repo we deliberately do not fork — on top of a **preview API** whose docs say
"APIs may change", and failure modes mitigated only by upstream prompt engineering. That is
three unsettled foundations for a feature whose user-visible payoff is "the conversation
stays live".

### The defect that was worth more than the feature

The two headline research features — the theorizer (5–15 min) and DataVoyager (20–40 min)
— **already** don't block a turn: they submit with `--no-wait`, return a `task_id`, and
leave the **client** to poll. This client never polled. That was not a missing panel:

> `persist_theory_outputs` and `persist_analysis_outputs` are called from the poll route
> and **nowhere else** (`backend/routes/artifacts.py:202,243`).

So a completed run wrote its results **nowhere**, while `prompts.py` instructs the
coordinator that "when a theorizer run completes, its theories are saved to the sandbox" —
and tells it to read them there on a later turn. Both headline features were quietly
losing their output in this client, and the agent was being told otherwise.

Polling therefore is not a display nicety. **It is the only thing that makes a finished run
durable.**

### What was built

The same user-visible payoff async subagents were meant to deliver — background work that
is observable and arrives on its own — using machinery that already exists upstream, with
no fork, no preview API and no new graphs.

- `Job` / `JobKind` decoded from the `values` snapshot, keyed on `task_id`. Fields taken
  from `HypothesisArtifactPayload` / `DataAnalysisArtifactPayload`
  (`backend/schemas.py:353,388`), not guessed.
- `Sidecar::watch_job` polls every **20s** on the Tokio runtime, which **outlives the
  turn**. Terminal states stop it — including `unavailable`, the subtle one: the thread's
  sandbox is gone, so no further poll can ever say anything and looping would burn requests
  forever.
- A **BACKGROUND JOBS** section above OUTPUTS, showing what is running, what it was asked,
  and *how long that kind of job usually takes* — a spinner with no expectation attached is
  indistinguishable from a hang.
- A finished job refreshes the spine, because the route has just written its results into
  the sandbox as it reported them.

Three details worth keeping:

- **Transport failures do not end a watch.** The sidecar may be restarting, or a turn may
  be saturating it; declaring a 40-minute job dead over one refused connection would be
  the worst possible failure.
- **The thread id is re-read on every poll**, not captured — "New thread" changes it, and
  polling the old one asks about a task that thread no longer knows.
- **A job with no `task_id` is never listed.** A completed artifact carries results but no
  id, and showing it as running would leave a spinner nobody could resolve.

### The lifetime question, answered narrowly

§14 flagged "the sidecar dies with the window" as blocking background work. This does not
need it solved: polling runs for as long as the window is open, and the job itself lives on
Asta's hosted service, recoverable by task id. Making the backend outlive the window is a
real design change with real costs (adoption, orphans, a second app instance) and it is not
required to collect a result the user is waiting for. **Deferred, not dodged** — closing the
window mid-job still means nobody persists that run.

**Unverified:** no long job has been run through this. The decode and route construction are
tested against the measured payload shapes, but the poll loop has never watched a real
theorizer run to completion.

## 30. Async subagents, without forking Mini-Me (2026-08-01)

Requested explicitly, with a fork authorised if needed. **It turned out not to be**, and
that is worth recording, because §14 had this filed as a structural upstream change.

### The three facts that made it an extension

1. **`AsyncSubAgent` is a reference, not an agent** — `{name, description, graph_id, url}`
   — and **`url=None` selects the in-process ASGI transport** (verified in
   `deepagents/middleware/async_subagents.py:_ClientCache.get_async`). No second server,
   no port, no credentials. The sync path *does* raise on `url=None`, so this only works
   because our stack is async throughout, which §18 already forced.
2. **`langgraph dev` accepts `--config PATH`**, and the desktop app builds the launch
   command. So extra graphs can be declared from the **client** side.
3. **`backend/agent.py:agent` is an async factory.** A second graph id can point at the
   same factory, so the background worker is a real Mini-Me with every tool and subagent
   it normally has.

### Why a background *coordinator*, not one graph per subagent

The obvious reading of the docs is "one graph per async subagent". Building those would
mean replicating `_build_runtime_subagents` — its MCP tool fetches, model resolution and
per-subagent middleware — inside our overlay. That is exactly the duplicated logic that
becomes merge debt the first time upstream touches it.

Delegating to a background **coordinator** reuses upstream's assembly verbatim, and is
strictly more capable: the worker can chain subagents, run its own analysis and write a
report. It works here specifically because execution is **local** (§19) — the background
worker shares the researcher's filesystem, so files it writes are simply *there*. Under
the remote sandbox each thread gets its own and the results would land somewhere the
user's thread cannot see.

### The pieces

- `overlay/minime_local/async_agents.py` — declares the async subagent (`url=None`),
  builds the background graph from upstream's own factory, and injects
  `AsyncSubAgentMiddleware` by wrapping `create_deep_agent`, the same patch point the
  approval gate uses (§18). Installed *after* approval, so the background worker inherits
  the same gate.
- `overlay/minime_local/make_config.py` — reads upstream's `langgraph.json` and writes
  `.mini-me-desktop.langgraph.json` beside it with the extra graph. **Beside it**, because
  every path in that file is relative to the file. **Extends rather than reconstructs**,
  because it carries `dependencies`, `env` and the `http` block that mounts the spine and
  job-poll routes. **Every launch**, because a copy generated once goes stale after a
  backend update.
- The launch joins them with `&&`, so a generator failure stops the launch instead of
  starting a coordinator whose tools point at a graph nobody serves.

### Two guards

**No recursion.** A background worker built by the same wrapped `create_deep_agent` would
be handed `start_async_task` too, and could spawn another, and so on — a runaway that
bills the user's model key. A `ContextVar` set while the background graph is built
suppresses the injection.

**Off by default.** It rests on a preview API whose docs say "APIs may change", and it
only functions when the generated config is in play. A coordinator holding tools for a
graph the server does not serve fails *mid-task, in front of the user* rather than at
startup, so `MINIME_ASYNC_SUBAGENTS` must be set and the Settings toggle
("Let work run in the background") is opt-in.

**Verified:** the config generator run against the real `langgraph.json` — `auth`,
`dependencies`, `env`, `http` and `python_version` all preserved, upstream's `agent` graph
untouched. The launch command is pinned by a test: generator before server, `--config`
present, and byte-identical to before when the toggle is off. 73 tests.

**Unverified — and this is the big one:** no background task has ever been started. The
wiring is measured but the round trip (`start_async_task` → the worker runs → results come
back) has not been exercised against a live backend.

## 31. Background work you can actually answer (2026-08-01)

Two gaps stood between §30's wiring and background work being usable. The first was not a
missing feature — it was a hang.

### The approval nobody could answer

The overlay wraps **one** `create_deep_agent`, so the background worker inherits the same
`execute` gate as the foreground agent. But the worker runs on its **own thread**, and the
client only ever resumed the conversation's (`sidecar.rs:173,246`).

So the first command a background task tried to run stopped it dead, waiting for a
decision nothing in the app could deliver. It would not have failed or errored — it would
have sat at "running" forever. Every data task (cleaning, EDA, analysis) hits `execute`;
only literature search and writing would have worked at all.

`GET /threads/{id}/state` answers everything needed in one call: its
`tasks[].interrupts[]` carry **exactly** the payload `decode_interrupt` already parses, so
a background approval and a foreground one are the same shape and render the same card.
Status is *derived* rather than reported — an interrupt means waiting, an empty `next`
with no interrupt means done, anything else is working.

Answering goes to `POST /threads/{that}/runs` with `resume_request_body` — the identical
body a foreground resume sends, so a change to the decision shape cannot fix one path and
leave the other broken.

**Not streamed into the transcript.** A background run's tokens are not the answer to
anything the researcher asked in the chat; mixing them in is how "what am I reading?"
happens. The Jobs panel reports it instead.

### Seeing the tasks

`async_tasks` is agent state, so it arrives in every `values` snapshot — no extra route.
Each entry gives `thread_id`, `agent_name`, `status` and the description. Three details:

- **`interrupted` is not terminal.** Treating it as finished would stop the watcher on the
  exact tick that needed a person. Terminal is `success`, `error`, `timeout`, `cancelled`.
- **Sorted by task id.** A map has no order, and the panel would otherwise reshuffle on
  every frame.
- **A stale snapshot never erases a pending approval.** The snapshot knows what the
  coordinator last recorded; the watcher knows what is true now. The card the user is
  looking at wins.

Watched every **4 seconds**, much faster than the 20s Asta job poll, because someone may
be sitting in front of the app waiting to say yes.

**Verified:** decode and terminal-state handling are tested against deepagents' own
`AsyncTask` field names. 75 tests, stable across three runs.

**Unverified:** no background task has been started, so no background approval has ever
been rendered or answered. The shape is measured; the round trip is not.

## 32. The Asta token expires every seven days — so the app mints it (2026-08-01)

**Reported symptom:** the theorizer failing repeatedly with *"The Asta theorizer returned
no task id — likely cause: missing or expired Asta access token"*, on a machine where
`asta auth print-token` worked perfectly.

Both were true. Decoding a real token: `exp - iat` = **604800 seconds — seven days**. So a
token pasted into Settings is a weekly chore, and when it lapses the failure names neither
the token nor the fix. Worse under WSL: being signed in on the *Windows* side proves
nothing, because the backend runs inside the distro.

`asta auth login` already leaves a **refresh** credential behind, and
`asta auth print-token --raw --refresh` turns it into a valid access token on demand. So
the app now mints one **per launch**, and the researcher signs in once.

Three details:

- **At spawn, not at window-open.** This can cost seconds on a cold WSL distro, and by
  then the user is already waiting on a backend start.
- **Shape-checked before use.** Without `--raw` the CLI pretty-prints a decoded header and
  payload; with nobody signed in it prints prose. Handing either to the backend as a
  credential produces an authentication failure that blames the wrong thing, so only a
  three-segment base64url JWT is accepted — asked *about* the value, never logging it.
- **Silent fallback.** No CLI, not signed in, a changed flag — the stored token still
  applies and the Setup pane reports the real problem separately.

The preflight check was upgraded to match: `command -v asta` said *installed*, which is not
*usable*. It now asks the CLI for a token, so an expired login is caught at the pane with a
**Sign in to Asta** button rather than in the middle of a research question.

**Verified:** the mint command run against a real CLI returns a 1015-character
three-segment JWT and nothing else; the non-raw form begins `JWT Header:` and is rejected
by the guard. The pane reports "installed and signed in". 76 tests.

### The gap signing in from the pane exposed

On Windows the **Sign in to Asta** button worked — browser, Auth0, "Authentication
successful", and the pane went to **6 ok**. The theorizer still failed.

Because the token is minted when the backend **starts**, and the backend had been running
since before the sign-in. Every check was green and the thing still did not work, which is
the worst state a diagnostic pane can be in: it was telling the truth about the machine and
the wrong thing about the session.

A successful sign-in now says so in the fix output — *"Close and reopen the app: the
backend reads your Asta sign-in when it starts."*

### The three holes, and why the backend mints its own token now

A restart did not fix it either. Passing the token in as an environment variable had
**three** separate holes, any one of which is enough:

1. **`_command_env()` is called once, in `__init__`.** The environment is a snapshot taken
   when a thread's workspace is built — so a token that arrives later never reaches a
   single command.
2. **`ensure_running` returns early when a backend is already healthy.** The app only
   minted while *spawning*, so attaching to a backend that was already up — including one
   orphaned by a previous session — skipped it entirely.
3. **On Windows it has to survive the crossing into WSL** via `WSLENV`.

`current_asta_token()` in the overlay removes all three: the backend asks the CLI itself,
in the same environment every other `asta` command runs in. If those can authenticate, so
can this. Cached for ten minutes so it is not a subprocess per command, and refreshed from
`_execute_with_token`, which is already inside `asyncio.to_thread` — `langgraph dev`'s
blocking-call guard rejects subprocesses on the event loop, and that guard has aborted a
run in this project before.

An explicitly set `ASTA_TOKEN` still wins, because someone who sets one means it. Anything
that is not JWT-shaped is ignored in both directions.

**Verified against the real CLI:** mints, is JWT-shaped, caches, prefers a supplied token,
and ignores junk in `ASTA_TOKEN` in favour of a minted one.

**Superseded:** have the overlay read the token from a small file at command
time rather than from the process environment, so the app can refresh it into a *running*
backend. `_command_env()` already reads `ASTA_TOKEN` at call time for exactly this kind of
reason — but it runs on the event loop, where `langgraph dev`'s blocking-call guard rejects
filesystem syscalls, so the read has to move somewhere off the loop first. That also fixes
the seven-day expiry landing mid-session rather than between launches.

### Showing who is signed in, and for how long

`asta auth status` reports everything worth surfacing, including — usefully —
**`Auto-Refresh: Enabled`**, which confirms the CLI refreshes its own access token and so
corroborates the design above. The Asta row now reads:

```
✓ Asta CLI    piero.palacios@cipotato.org · token 167h 55m left
```

Two reasons that is worth the parsing:

- **Which account.** On a shared machine, or after someone signs in with a personal
  address by mistake, "signed in" gives no way to work out why permissions look wrong.
- **How long.** Seven days is short enough to matter and long enough to forget.

The **Sign in again** button is offered even when the row is green: when the *refresh*
credential finally lapses — not the access token, which now renews itself — that is the
only cure, and a button that only appears once you are already broken is a button you
cannot find.

The parser splits the Rich table on `│` rather than matching prose, and is used **only to
enrich a row that already passed**, so a change to the CLI's formatting costs a label and
never a check. A test pins it against the real output verbatim, box-drawing and all.

### 32b. It was never the token — it was the account (2026-08-01)

After the minting fix, the theorizer still failed. Decoding the two access tokens the user
had produced, side by side, settled it:

| | `auth0\|69fe…` (cgiar.org) | `google-oauth2\|1142…` (cipotato.org) |
|---|---|---|
| permissions | `access:all_endpoints` | `access:all_endpoints`, `access:biopathways`, `enroll:asta_integration`, **`enroll:theory_generation`** |

The theorizer requires **`enroll:theory_generation`**. The account signed in inside WSL was
the first one. Its token was present, valid, server-verified and **not entitled** — and
upstream reports that as *"no Asta task ID was returned, which usually means the access
token is missing or expired"*.

That message is a guess, and being a *plausible* guess is what made it expensive: it sent
the user to re-authenticate, repeatedly, for something re-authenticating could never fix.
Two rounds of work here — minting the token, then reading it at command time — were both
real improvements aimed at the wrong target.

**The check now reads the claims.** `asta auth print-token` *without* `--raw` prints the
decoded payload, permissions and all, so no JWT decoding of our own is needed. An account
that lacks the permission gets a warning that says so in those words, plus the sign-in
button pointed at the account that has it.

The lesson worth keeping: *"signed in"* was never the question. **Entitled** was. A
diagnostic that reports authentication and calls it authorization will confidently send
someone the wrong way, which is worse than reporting nothing.

### 32c. Opening the sign-in page where the browser actually is

The **Sign in to Asta** button worked, but the real output showed why it was awkward:

```
gio: https://auth0.allenai.org/activate?user_code=DPMW-BJCG: Operation not supported
```

`asta auth login` prints its device-activation URL and then tries to open a browser — from
**inside the distro**, which has none. The sign-in only completed because the user opened
the link by hand.

The pane now catches the URL out of the streamed output and offers **Open the sign-in
page**, plus a copy. The opener deliberately does **not** go through `shell_argv`: routing
it into WSL is precisely what already fails. On Windows it is `cmd /C start "" <url>` —
with the empty title argument, without which `start` treats a quoted URL *as* the title and
opens nothing.

Prominent, and above the log, because while it is showing the command is **blocked**
waiting for someone to visit that page.

**What is deliberately not done: saving the token.** The obvious next step — "log in once,
store the token" — is the thing three rounds of debugging just removed. Access tokens last
seven days; a stored one is stale by definition, and `_command_env()` captured it once per
workspace anyway. The overlay asks the CLI for a fresh token every ten minutes instead
(§32), and the CLI's own `Auto-Refresh: Enabled` does the renewing. There is nothing to
save that would not immediately start rotting.

**The test earned its place immediately:** the first version left the trailing colon on
`gio: <url>:` — a character worth stripping only because the real line has it there.

**And the code gets its own line.** Seen on Windows: the log box showed
`| Visit: https://auth0.allenai.org/activate?user_code=KFDM-BQQG |` **clipped at the pane's
edge**, with the text unselectable. A URL is a single unbreakable word — it cannot wrap at
420px, and what falls off the end is the device code, the one part a person has to read and
type. It is now extracted and shown large on its own line, above the link buttons.

**And the actions moved out of the scroll area.** The buttons were children of the log box
— a flex child, therefore shrinkable — so it squeezed until "Open the sign-in page" was
sliced in half and unreadable. A button you cannot read is worse than no button: the user
can see something is there and cannot use it. The block now holds the header, the code and
the buttons at a fixed size, and only the output lines scroll beneath them.

Three rounds on one small panel, each found only by looking at a screenshot. Rendering is
not something this project can reason its way to from a Linux box.

## 33. The fix that never reached the machine (2026-08-01)

Two rounds of Asta token work — minting it, then reading it at command time — and the
theorizer still failed. Neither had ever run.

**The backend loads the overlay copy inside the distro.** §25 made the launch prefer
`<checkout>/.desktop-overlay`, and that was right: it removed host execution's dependence
on `/mnt/c` being reachable, which fails *silently*. What went unnoticed is that the copy
is made at **provisioning** time. So `git pull` + `cargo build --release` updated the
repo's `overlay/`, the app relaunched, and the backend went on importing a copy from days
earlier. Every overlay change since provisioning was invisible.

This is the worst shape a bug can take: the fix was correct, shipped, and verified on the
dev box, and the user watched the same failure three times. It also quietly invalidated the
verification — "verified against the real CLI" was true of code that was not running.

**Every launch now syncs it.** Three small files, so copying them unconditionally is
cheaper than working out when to. `|| true`, because a stale overlay still beats a backend
that will not start, and the repo's copy may be genuinely unreachable — the case the
in-distro copy exists for. Ordered before the server, and independent of the async-subagent
toggle, which had been gating the only other pre-launch step.

**Verified in real bash:** a stale provisioned copy is replaced with the current one, and
an unreachable source exits 0 so the launch continues. A test pins the ordering and that
the sandbox path stays untouched.

**The general lesson.** Anything the app *installs* onto the user's machine is a second
copy with its own version, and needs a story for how it gets updated. The overlay had none.
Worth checking the others: the generated config regenerates each launch (§30) and the
bundled backend is refreshed by re-provisioning — but `vendor/Mini-Me` inside a shipped
bundle has the same shape of problem, and click-to-update is still unbuilt.

### 33b. And then the fix itself was wrong

With the overlay finally syncing, the code ran — and failed immediately:

```
submit failed: 'LocalWorkspaceBackend' object has no attribute 'env'
```

`_execute_with_token` refreshed `self.env`. deepagents calls it **`self._env`**
(`LocalShellBackend.__init__` builds it; `execute` passes it to the subprocess). Guessed,
not checked — and because the refresh sits on the path *every* command takes, a wrong
attribute name turned "the theorizer has no token" into "nothing executes at all".

Now reached with `getattr(self, "_env", None)` and an `isinstance` check. If a later
deepagents renames it we lose the token refresh, which is a degradation; taking `execute`
down with it is not.

**Verified against the real class**, in the backend's own venv: `_env` exists, `env` does
not, and a token written into it is visible to the command that runs.

Two lessons, both cheap in hindsight. **Private attributes of a pinned dependency are
fair game, but only after looking** — this file already reads upstream internals
deliberately (`_truncate_execute_response`), and each one was checked except this. And
**anything on the universal path needs a failure mode that is a degradation**, because
its blast radius is everything.

## 34. A PATH problem wearing an authentication costume (2026-08-01)

With the overlay finally syncing and the attribute name fixed, the theorizer *still*
reported a missing or expired token — on an account whose token the Setup pane showed as
valid for 167 hours, with `enroll:theory_generation`, and whose exact submit command
returns a task id when run by hand.

**`execute` runs commands through `/bin/sh` with exactly the environment we hand it.** Not
a login shell — so `~/.profile` never runs, and `~/.local/bin` is not added. That is where
`uv tool install` puts the **asta CLI**. If the backend's own PATH happens to lack it,
every `asta` command exits **127, `sh: asta: not found`** — and upstream reports that as
*"no task id was returned, which usually means the access token is missing or expired."*

Proven directly against `LocalShellBackend`:

```
without ~/.local/bin : /bin/sh: 1: asta: not found   (exit 127)
with    ~/.local/bin : asta, version 0.101.1
```

`_command_env()` now puts it on PATH. And the Setup pane could not have caught this: its
probe runs `bash -lc`, a **login** shell, which reads `~/.profile` and finds `asta`
perfectly — a check that passes where the thing being checked would fail.

### The change that should have come first

`_log_failure` now writes any non-zero command, with its output, to the sidecar log.

Tools discard what a command actually printed and substitute their own summary. The
theorizer's is a *guess*, and a plausible one — which is precisely what made it expensive:
it named a cause, so nobody looked further. It sent this project through minting a token,
reading it at command time, syncing the overlay and chasing account entitlements, while
the real message — five words, `sh: asta: not found` — was being thrown away at every
step.

**The lesson worth keeping:** when a component reports a *cause* rather than an *error*,
the first move is to recover the real output, not to act on the guess. Four fixes here were
individually correct and aimed at a diagnosis nobody had verified.

## 35. The stale token that failed silently (2026-08-01)

The sidecar log finally settled it, and the culprit was our own precedence rule.

Reproduced directly against the CLI:

```
ASTA_TOKEN=<valid>   asta generate-theories … --no-wait  →  exit 0, a task id
ASTA_TOKEN=<stale>   asta generate-theories … --no-wait  →  exit 0, EMPTY OUTPUT
```

**The CLI prefers `ASTA_TOKEN` over its own stored credentials, and fails silently when
it is bad** — exit 0, nothing on stdout, nothing on stderr. Upstream then reports "no task
id was returned, which usually means the access token is missing or expired": correct
about the cause, useless about the source. And an exit-0 failure walks straight past the
failure logging added in §34, which is why that produced nothing.

`ASTA_TOKEN` reaches the backend from the **OS keychain**, where a token pasted days
earlier was still sitting. §32 had decided "an explicitly supplied token always wins —
someone who set it meant it". That reads well and is wrong: a value in a keychain from
last week is not an intention, and preferring it silently disabled every Asta tool.

**Inverted.** The CLI is the authority — `asta auth login` leaves a refresh credential and
the CLI renews itself, so a minted token is always at least as good as a stored one. A
supplied value is now tried *only* when nothing can be minted, and says so loudly when it
is used.

### Why this took so long

Six rounds, each a real defect, none of them this one:

| | what was wrong | why it looked right |
|---|---|---|
| §32 | token minted only at spawn | the error said "expired" |
| §32 | read once per workspace | ditto |
| §32b | account lacked `enroll:theory_generation` | two accounts genuinely differed |
| §33 | the overlay copy was months old | the fix was correct, just not running |
| §33b | `self.env` guessed, not checked | crashed loudly, so looked like *the* bug |
| §34 | `~/.local/bin` missing from PATH | reproduced exit 127 exactly |

Every one deserved fixing. But the diagnosis driving them came from a tool that reports a
**guess** as a cause, and a CLI that fails with **exit 0 and no output** — a combination
that defeats both "read the error" and "log the failures". The step that actually worked
was reproducing the command by hand with a deliberately bad token.

**The lesson:** when a component reports a cause rather than an error, do not act on it.
Reproduce the failing call directly, and vary one input at a time — including the ones the
app itself supplies.

### Verified on Windows, end to end (2026-08-01)

`genera una teoria de como se forman los rayos` → the theorizer **submitted**
(`845f8553-499c-4ea8-a3e4-6540101cb39d`), and the **BACKGROUND JOBS** panel showed it
running with its question and expected duration. Two features proved out at once: the
theorizer itself, and §29's job watching — which had never seen a real long job.

Still to observe: the completion. The poll route persists theories into the workspace on a
terminal state (§29), so the job should turn green and the spine refresh **without another
turn**. That last link is the one that was silently broken before any of this.

### 30b. Registered, but never handed over (2026-08-01)

First live test of background work: the coordinator answered *"lanza esto en segundo
plano"* by delegating to `academic_researcher` — the ordinary, **blocking** subagent — and
the chat froze for the whole literature search. Exactly what the feature exists to prevent.

`MINIME_ASYNC_SUBAGENTS` is what `async_agents.install()` checks before adding the
middleware, and **nothing set it**. The graph was registered (the log shows
`Importing graph profiling … graph_id=background`), the config generation worked, and the
coordinator was never given `start_async_task`. So it used the only delegation it had.

Two halves of one feature, and only one of them was wired: a Settings toggle that
generated the config but never enabled the tools. The toggle *looked* like it worked
because the visible half — the extra graph — did.

Set now via `feature_env`, deliberately **not** folded into `execution_env`: that returns
nothing at all for the remote sandbox, so combining them would silently disable background
work under `--sandbox`. The two settings are independent, and are kept that way.

A test asserts the variable is in the launch when the toggle is on and absent when it is
off — registering the graph is only half of it.

## 36. P6.5 verified on Windows (2026-08-01)

**Background work runs, and the conversation stays live.**

- `· start_async_task` fired, control came back immediately, and **two background workers
  ran concurrently** while the researcher kept typing. That is the payoff §14 justified
  going native for, delivered without forking Mini-Me (§30).
- The **Theorizer completed by itself** — `✓ Theorizer · completed` in BACKGROUND JOBS,
  with `sources · 11` and `hypotheses · 1` in OUTPUTS, no second turn asked for. That
  closes §29's loop end to end: poll → terminal state → **persist**, the step that was
  quietly doing nothing before any of this and losing every long run's results.
- A **SUGGESTED NEXT** card offered "Synthesize theories from the literature", derived from
  artifacts the run had just produced.

One correction to §30's design note, from watching it work: the background worker is a
*whole coordinator*, and the concern was that this made each task heavier than a
single-purpose subagent. Running two at once cost nothing visible, and the flexibility —
one worker doing search, another doing synthesis — is what made the test easy to write.

### Still unverified

**Background approvals** (§31). Both test tasks were literature work, which never touches
`execute`, so the gate was never reached. The next test has to run code — a task that
writes and analyses a dataset — because until that path is confirmed, any background task
touching data may simply hang.

**Completion of an async task.** The theorizer's completion is confirmed; a *background
worker* finishing and returning its result is not.

## 37. Background runs carry no key (2026-08-01)

Both background workers failed with *"The async subagent encountered an error"* — no
further detail. The Jobs panel caught it correctly (`✗ background worker · error`), which
is §31 working; the cause was structural and ours.

```python
run = client.runs.create(
    thread_id=thread["thread_id"],
    assistant_id=spec["graph_id"],
    input={"messages": [...]},        # ← no `config`
)
```

The middleware starts the run with **no config at all**. This app's whole key design is
that the model choice and API key travel *in the run request* (§20/§22) — which works for
turns we create, and cannot work for a run the backend starts by itself. So the background
run had neither `model_config` nor `__llm_keys`, upstream fell back to the WorkOS vault
(the `404` visible in the log all along), found nothing, and could not construct a model.

**Fixed by putting the key in the backend's environment**, which LangChain reads when no
key is passed — `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, and so on per provider.

**Only when background work is switched on.** With it off nothing needs this and the key
stays out of the environment entirely, which is the stronger posture and remains the
default. That is a real trade being made deliberately: enabling background work moves the
key from "request-only" to "also on the backend process", the same standing `ASTA_TOKEN`
has always had.

Worth noting what this says about §30's "no fork needed" claim, which still holds — but
the seam is thinner than it looked. Co-deploying the graph was free; the *run creation* is
upstream's, and anything the request has to carry is out of our reach. A wrapper around
`start_async_task` in the overlay could inject config properly, and is the better fix if a
second such gap appears.

## 38. The real reason background work failed — and it was never the key (2026-08-01)

§37 was right about the mechanism and wrong about the consequence. `client.runs.create()`
does pass no config, but the missing key was the *smaller* half of what that costs. The
larger half is on the very next line of our own code:

```rust
// protocol.rs — the config every foreground turn sends
"recursion_limit": 10_000,
```

with the comment we wrote ourselves months ago: *"LangGraph defaults to 25 supersteps, and
one turn already spends ~22 on middleware alone before any delegation."* A background
worker **is a whole Mini-Me coordinator** (§30). Started with no config it gets the default
25, spends ~22 on middleware, and dies before doing any work — on any provider, with or
without a key. That is why §37's fix changed nothing.

### Wrapping `start_async_task`, as §37 predicted

The overlay now replaces exactly one of the middleware's five tools. The other four
address a run by id and need nothing we have. The replacement reads the *live* run's config
via `langgraph.config.get_config()` and forwards it onto the background run:

```python
FORWARDED_CONFIG_KEYS = ("model_config", "__llm_keys", "__is_for_execution__")
```

An **allowlist, not a copy** — `configurable` also holds `thread_id`, `checkpoint_ns` and
`run_id`, and forwarding those would point the background run at the conversation's own
thread. Verified against a fake client: the run is created on the `background` graph with
the researcher's model, their key, `recursion_limit: 10000`, and no trace of the chat's
thread id.

This is strictly better than §37's environment variable, which is now **reverted**:

- it carries `base_url`, so a `custom` (OpenRouter/Groq/Ollama) endpoint works — no
  environment variable can express that;
- it uses the model the researcher actually picked, rather than upstream's
  `MINIME_DEFAULT_MODEL` fallback of `openai::gpt-5.4` (`backend/models.py:24`);
- it keeps the key **out of an environment the agent's own `execute` tool can read**.

So the key stays request-only whether background work is on or off, and §37's "deliberate
trade" is withdrawn — there was no need to make it.

### The placeholder that cost two rounds

*"The async subagent encountered an error"* is not upstream being unhelpful; it is upstream
having nothing to say:

```python
error_detail = run.get("error")
result["error"] = str(error_detail) if error_detail else "The async subagent encountered an error."
```

The dev server records no `error` on the run record, so that branch always fires. The real
text **is** available — on the thread's pending task, which `/threads/{id}/state` returns
and this app was already fetching for approvals. `thread_state` now reads it, and the Jobs
panel shows the exception line instead of the word "error".

That fixes a second defect found while looking: the watcher derived `success` from an empty
`next`, and a *failed* run leaves its task pending — so `next` is never empty and a dead
worker read as **running forever**. It only ever showed "error" because the researcher
happened to ask, which routed through `check_async_task`. Failure now beats every other
signal in that derivation.

This is the same lesson as §35, and it has now cost this project twice: **when a component
reports a cause rather than an error, go get the real output before fixing anything.**

## 39. Background work had never run once (2026-08-01)

The cause of every failed background task, from the first:

```python
async def background_graph():          # our factory — no parameter
    return await upstream_agent()      # TypeError: missing 1 required positional argument: 'config'
```

`backend/agent.py` declares `async def agent(config: RunnableConfig)`. Our factory took no
parameter, so it had none to pass on. Every background run raised `TypeError` while the
graph was being **constructed** — before a single node executed.

That also explains the thing §38 could not: why there was no error text to read anywhere.
A run that dies during construction writes no checkpoint, so `/threads/{id}/state` has no
task to hang an error on. The middleware's placeholder was genuinely all that existed.

**Fixed** — the factory takes `config` and passes it on. Verified three ways rather than by
inspection, since inspection is what missed it twice:

- against the dev server's own classifier, `_classify_factory(background_graph)` now
  resolves to `{"config": <RunnableConfig>}` (`langgraph_api/_factory_utils.py`);
- the graph builds — a real `CompiledStateGraph` with the full middleware stack, where
  before it raised;
- the built worker holds `execute` but **no** `start_async_task`, so `_BUILDING_BACKGROUND`
  still stops a worker spawning workers.

The call is adaptive (`inspect.signature`) rather than hardcoded: if upstream ever drops
the parameter this keeps working, and it warns instead, because the failure mode of getting
it wrong is invisible.

### On the two fixes that came before this one

Neither §37 nor §38 was the cause, and it is worth being exact about what they were:

- **§37 (key on the environment) was simply wrong** and is reverted. It addressed a real
  gap with the wrong mechanism.
- **§38 (forward the config) was a real bug and is still required** — it is what makes the
  `config` this factory now receives contain the researcher's model and key. It was also
  the *next* failure in line: the recursion limit would have killed the worker at superstep
  25 the moment the graph built.

The pattern in all three: a placeholder error was treated as evidence. §35 recorded the
lesson once — *recover the real output before fixing anything* — and it was not applied,
because the "real output" here was never going to appear in a log the run never reached. The
sharper rule: **when there is no error text anywhere, suspect the constructor, not the run.**

## 40. Background work verified end to end (2026-08-01)

`✓ background worker · success` on Windows, and the **approval gate fired** — the path §31
built and that nothing had ever exercised, because both earlier tests were literature work
that never touches `execute`. A background worker now generates data, asks permission on
its own thread, and the answer reaches it. P6.5 is done.

One defect surfaced the moment it worked: the approval card grew with the command. An
agent-written script is hundreds of lines, the card took all of it, and Approve/Reject —
along with the composer beneath them — were pushed off the bottom of the window. A gate
whose buttons cannot be reached is worse than no gate: it hangs the task and hides why.

The command now scrolls inside a capped region and the decision sits outside it, in **both**
cards — the foreground one and the Jobs-panel one, which has the same failure at a narrower
width. This is the third time the fix has been *"actions outside the scroll area,
`flex_none` on anything that must not be squeezed"*, and the third time it was found from a
screenshot rather than from the code.

## 41. Approval scope, widened on the researcher's evidence (2026-08-01)

*"I need to click too many times approve. maybe thats something scientist will dislike."*

Three separate gaps, only one of which was the missing button:

1. **"Approve the rest of this turn" already existed — and was unreachable.** It sat below
   the command, and §40's card overflow pushed it off the window. The feature had been
   there for weeks and had never been seen.
2. **Background tasks had no blanket option at all.** A worker asks once per command over
   several minutes, on its own thread, while the researcher has gone back to work. That is
   the worst place to require a click each time, and it is precisely where handing work to
   the background stops being useful.
3. **Turn scope was too small.** One analysis is a dozen commands across several turns.

Added: **"Approve everything in this conversation"** (covering background workers too) and
**"Approve the rest of this task"** on the Jobs-panel card.

### What keeps this from becoming a rubber stamp

§19's original argument still stands — *"the tenth identical dialog in one analysis is not
read, it is dismissed, and then neither is the eleventh — which is the one that mattered."*
The answer to that is not to make people click more; it is to make the grant **bounded,
visible and revocable**:

- **Never persisted.** Nothing is written to disk. Closing the app ends it.
- **Ends with the conversation.** "New thread" clears both the conversation grant and every
  per-task one.
- **Visible the whole time it holds.** The status bar shows *"approving everything — click
  to stop"* in accent colour whenever it is in force. A blanket grant that is invisible is
  the actual hazard.
- **Revocable in one click**, without starting a new conversation — otherwise "just this
  once" becomes permanent through inconvenience.
- **Still recorded.** Auto-answered commands still appear in the card and the trace. This
  removes the interruption, not the record.

The permanent, cross-session version remains what it was: a Settings toggle the user has to
go and find, worded *"Off is for automation, not a recommendation."*

## 42. Outputs a researcher can actually see (2026-08-01)

Three requests, one root cause: **the app did not know where the agent's files went.**

The backend writes each thread's files to `~/.mini-me/workspaces/<thread>` — inside the WSL
distro, which on Windows means `\\wsl.localhost\Ubuntu\home\<user>\…`. For a user base that
is ~98% Windows and none of whom are expected to code, files they cannot find are files
that do not exist.

So the app now **chooses** that directory instead of letting the backend default, and puts
it on the Windows side: `Documents\Mini-Me\<thread>`, passed in as `MINIME_LOCAL_WORKSPACE`.
All three requests fall out of that one decision:

1. **"A button to download all the documents."** There is nothing to package — the files
   are already in the researcher's own Documents. *Open this conversation's files* in the
   OUTPUTS panel opens the folder in Explorer.
2. **"Generated plots should show in the chat."** The app can now read them. Figures appear
   under the answer that produced them, capped at 420px, and clicking one opens it full
   size. They are found by **diffing the workspace across the turn**, not by being
   reported: a plot is written by a `matplotlib` script inside `execute`, which registers
   no artifact and tells the client nothing. The file appearing on disk is the only signal
   that exists.
3. **"I cannot see which subagent is doing the job."** Separate fix: `thread_state` now
   reads the last tool call off the worker's own thread, so the panel shows
   `running · academic researcher` rather than `running` for ten minutes. `task` calls
   report the subagent they delegated to; everything else reports the tool.

The cost of the move is that writes cross WSL's 9p mount, which is genuinely slow for
*many small* files — it is why the backend venv stays inside the distro (§25). A turn's
outputs are a handful of CSVs, figures and reports, and being able to find them is worth
more than the milliseconds.

**Migration:** files written before this change stay where they are, under
`~/.mini-me/workspaces` in the distro. Nothing is moved or deleted; new conversations use
the new location.

Also verified this round, and long outstanding: **the Windows Job Object works.** After
closing the app, `wsl -- pgrep -af "langgraph dev"` prints nothing — the backend dies with
its parent, so no orphaned server holds the port (§26).

## 43. Two bugs the plots exposed, and a UI debt worth naming (2026-08-01)

### The background worker was writing where nobody looked

A screenshot showed the coordinator running `ls`, `ls`, `ls`, `glob` ×8, `read_file` ×3 and
then admitting *"the files weren't at the root path I first tried"* — before printing three
absolute paths as text. No figure rendered.

The cause: **a background worker runs on its own LangGraph thread**, and the workspace is
one directory per thread (`workspace.py`). So the worker wrote to *its* directory, while
the app looked in the conversation's and the coordinator looked in its own. Three
components, three different folders, and the only one that could find anything was the
worker itself.

Fixed by pinning the worker to the conversation's workspace: `start_async_task` forwards
`__workspace_thread__`, and `LocalWorkspaceBackend` prefers it over the run's own thread
id. Note this is deliberately **not** forwarding `thread_id` — that would point the run at
the wrong thread and corrupt it; this is a separate key read only when choosing a
directory. An existing pin wins, so a worker started by a worker still writes to the
conversation's folder.

### Plots were diffed against the wrong moment

§42 snapshotted the figures at turn *start* and diffed at turn *end*. A background worker
finishes on its own schedule — usually between turns, sometimes minutes after the turn that
started it — so its figures fell outside every window and were never attached.

Now the diff is against **what the transcript already shows**, which makes `collect_plots`
safe to call from anywhere; it also runs when a background task completes.

### P6.7 — take the UI seriously

Stated plainly by the person using it: *"our current app is really awful hehe."* That is
fair, and it is not a mystery — every panel here was built to prove a mechanism worked, and
none was built to be looked at. Buttons are hand-rolled `div()`s with eight style calls
copy-pasted per site, which is exactly why they drift.

**What is actually borrowable.** GPUI *is* Zed's framework, but Zed's `ui` and `theme`
crates are monorepo-only — unlike `gpui` itself they are not published. So this is adopting
**patterns**, not adding dependencies. `gpui 0.2.2` already ships the primitives needed:
`svg`, `uniform_list`, `list`, `anchored`, `deferred`, `canvas`, `div().tooltip()`,
`ScrollHandle`, animation and an image cache — almost none of which this app uses.

In rough value order:

1. **Visible scrollbars.** `overflow_y_scroll` draws nothing, so content that scrolls looks
   like content that is cut off — the direct cause of "I cannot go to the bottom to approve
   or reject" (§40). Zed draws its own; so should we.
2. **A `Theme` struct with semantic roles** (`text`, `text_muted`, `border`,
   `element_hover`, `status_error`) replacing the scattered `const` hex in `main.rs`. One
   source of truth, and the precondition for a light theme.
3. **A component vocabulary** — `Button`, `IconButton`, `Label`, `Divider`, `Tooltip` — so a
   button is one call, not eight, and every card looks the same because it *is* the same.
4. **Bundle a font.** We ship none, so fenced code renders in Segoe UI. Register a mono at
   startup via the text system.
5. **Icons as `svg()`** tinted by the theme, replacing the text glyphs `◐ ✓ ✗ ◎`.
6. **Tooltips** — the framework has them; this app uses none.
7. **`uniform_list` for the transcript.** Every message is currently laid out every frame;
   a long session will crawl.
8. **Focus rings and a tab order.** One focusable field today, and no visible focus.
9. **Resizable/collapsible panels.** The right panel is a fixed width nobody chose.
10. **Toasts** instead of one status line that overwrites itself.

Deliberately *not* on this list: **text selection**, which needs a custom element and is the
one thing here GPUI genuinely makes hard (§16).

## 44. Why the approval button appeared to move (2026-08-01)

*"Sometimes the button approve for all the conversation appears bottom and sometimes in the
background panel."*

It was not moving. There are **two gates**, and which one you see depends on *who* needs
permission:

| Who asks | Where it appears |
|---|---|
| The coordinator, on the conversation's thread | The card above the composer |
| A background worker, on **its own** thread | The BACKGROUND JOBS panel |

That distinction is load-bearing — a background worker runs on a different thread and must
be answered there (§31) — but it was invisible, and the two cards offered **different
grants**: the chat card had *rest of this turn* and *everything in this conversation*, the
panel had only *rest of this task*. So the same intention had a different button, with
different wording, depending on which component happened to ask. That is indistinguishable
from a button that wanders.

Fixed by offering the conversation-wide grant in **both** places, worded identically. It
means the same thing in both — it is one flag — so wherever the researcher meets it, one
click ends the interruptions everywhere, foreground and background alike, until the
conversation ends or they click *stop* in the status bar (§41).

The narrower grant stays contextual, because its scope genuinely differs: *this turn* only
exists in the chat, *this task* only exists in the panel.

## 45. The download link (2026-08-01)

The largest remaining gap was not a feature. Installing meant `git clone` and
`cargo build` — which excludes, precisely and completely, every researcher this was built
for. `package.sh` already produced a folder; nobody could get at it.

`scripts/release.sh` closes that: it validates the bundle, zips it, checksums it, writes
release notes aimed at the person downloading, and creates the GitHub release.

**Version is now `0.1.0`**, not `0.0.0` and deliberately not `1.0`: WSL provisioning has
never run on a machine that never had WSL, and the executable is unsigned.

### It refuses to publish a bundle that would waste someone's afternoon

Each check is a way a colleague's first ten minutes get destroyed, and hearing about it
from them is the expensive way to find out:

- **a Linux binary in a Windows bundle** — easy to do from WSL, and useless to the entire
  audience;
- **`vendor/Mini-Me` missing** — the installer then asks for a GitHub token for a *private*
  repository, which is the exact wall the bundle exists to remove (§25);
- **`overlay/` missing** — host execution silently would not work;
- **a dirty working tree** — the tag would not describe what is in the zip.

`--dry-run` runs all of it, builds the zip, and prints the release note without touching
GitHub. Verified here end to end against a simulated Windows bundle: guards fire, the zip
builds, the checksum computes, the note reads correctly.

### Draft by default

Nobody has ever installed this app from a zip. A draft can be deleted; a published release
someone already downloaded cannot. Publishing is one printed command, to be run *after*
installing from the zip on a machine that is not the build machine.

### SmartScreen is addressed with words, since it cannot be addressed with a certificate

Unsigned, Windows shows *"Windows protected your PC"* with only a **Don't run** button
visible — and most researchers stop there, which would make every other fix in this
document irrelevant. Both the release notes and the bundled `README.txt` now say exactly
what will appear and which two words to click. Signing remains an organizational decision;
this is the honest interim.

### Not done: CI

A `windows-latest` workflow would remove the "build it on your own machine" step, but it
needs a token for the private backend repo and a Windows SDK with `fxc.exe` for the release
build (§26) — neither testable from here. A script that works today beats an untested
workflow that might.

## 46. Three shells, three spellings of a path (2026-08-01)

The first real release attempt died at the zip:

```
Compress-Archive : La ruta de acceso "\mnt\c\Users\...\dist" no existe
```

`release.sh` assumed **Git Bash** and reached for `cygpath`. It was run from **WSL**, where
`cygpath` does not exist, so the fallback handed PowerShell WSL's own spelling of the path —
which Windows cannot resolve. Neither shell ships `zip`, so the PowerShell branch is the one
that actually runs on the target machine, and it was the untested one.

Three shells, three spellings of the same directory:

| Shell | Path |
|---|---|
| WSL | `/mnt/c/Users/…/dist` |
| Git Bash | `/c/Users/…/dist` |
| PowerShell | `C:\Users\…\dist` |

`win_path()` now tries `wslpath -w`, then `cygpath -w`, then a transform that handles both
POSIX forms — verified against all three. It translates the **parent directory**, because
`wslpath` resolves paths that exist and the zip does not exist yet.

Worth naming the testing gap honestly: `--dry-run` passed here because Linux *has* `zip`, so
the branch that fails on Windows was never reached. A dry run that exercises a different code
path than the real one is not a dry run of the real thing — the guards it did verify were
real, but this one it structurally could not.

## 47. P6.7 begins: showing an agent's work without burying its answer (2026-08-01)

A screenshot of a real turn:

```
· read_file  · read_file  · read_file  · ls  · ls  · ls
· glob ×8 (as eight separate lines)  · ls  · ls  · read_file ×3
The files weren't at the root path I first tried...
```

Twenty-plus lines of process, then the answer — below the fold. The activity trace was
built in §15 to close a "silent gap" during long delegated turns, and it did. But it was
written for a turn that *works*; a turn that struggles emits the same call repeatedly, and
nothing folded it.

### What the field has already settled

Researched rather than guessed, and three sources converge:

- **Zed's own agent panel** proposes rendering each spawned subagent as a **collapsible
  sub-thread** — in the parent, one compact card (*"→ Subagent … (12 tool calls, 4.2s)"*)
  rather than the whole inlined timeline
  ([zed#57481](https://github.com/zed-industries/zed/discussions/57481)).
- A second Zed proposal names the temporal pattern exactly: **expand a turn live while the
  agent works, then collapse it on completion**
  ([zed#58314](https://github.com/zed-industries/zed/discussions/58314)).
- Luke Wroblewski's survey of shipped agent UIs reports the same lesson learned the
  expensive way: fully-disclosed tool calls proved *"too much information"*, and iterations
  *"focused on reducing the visual weight of tools and showing less process by default"* —
  the more tools an agent has, the more this matters
  ([lukew.com](https://lukew.com/ff/entry.asp?2142=)).

This app had already arrived at half of it: subagent groups have been collapsible with a
`▸ name · N steps · N chars` header since §15c. The **coordinator's own steps** were the
part still rendered flat and unbounded.

### What changed

- **Consecutive identical steps fold** — `glob ×8` on one line. Only *consecutive* runs:
  `read_file ×3, ls ×2, read_file ×3` is a different story from `read_file ×6`, and
  flattening the order would erase it. Applied to subagent steps too.
- **The coordinator's steps get the same disclosure the subagents had** — a
  `▾ 24 steps` header, open while the turn runs, **closed the moment it ends**. During a
  two-minute wait the steps are the only sign of progress; afterwards the answer is the
  point.
- *Expand / collapse agent activity* in the palette now reaches these too, rather than
  leaving half the activity shut.

Still ahead in P6.7, unchanged in priority: **visible scrollbars** (the highest-value single
fix), a theme struct, and a component vocabulary.

## 48. P6.7: a palette, a history, and files you can reach (2026-08-01)

*"In general looks awful."* Four things named: a colour palette, conversations in the left
panel with renaming, how to show a thread's files, and interactivity. Researched first —
the sources are cited where they changed a decision.

### The palette was one orange doing six jobs

`main.rs` held seven `const`s, and the brand orange was used for section headings, links,
buttons, the running mark, the host-execution warning and any border wanting attention.
When everything is emphasised nothing is. That, more than any individual panel, is why the
app read as amateur.

`theme.rs` replaces them with **roles**, on the model every dark-mode system converges on
([Muzli](https://muz.li/blog/dark-mode-design-systems-a-complete-guide-to-patterns-tokens-and-hierarchy/),
[Imperavi](https://imperavi.com/blog/designing-semantic-colors-for-your-system/)):

- **A surface ladder** — `BG` → `SURFACE` → `RAISED` → `OVERLAY`, so panels, rows and
  popovers separate by *elevation* instead of by drawing more borders.
- **Three text weights**, all AA: `TEXT`, `TEXT_MUTED`, `TEXT_FAINT`.
- **Status colours of their own** — `SUCCESS`, `WARNING`, `ERROR`, `RUNNING` — so "finished"
  and "clickable" stop looking identical.
- **One rule for the accent: orange means you can act on this, and nothing else.**

Two tests enforce it rather than trusting the eye: one computes WCAG contrast for every
ink/surface pair and fails below 4.5:1, the other asserts the ladder actually ascends and
that hover lifts. **The contrast test failed on first run** — `ERROR` on `RAISED` was
4.30:1, and `TEXT_FAINT` 4.17:1 — so both shipped values are ones the build chose, not ones
that looked fine.

### The conversations existed all along

The left rail was 64px holding one glyph. Meanwhile the backend had stored **every thread
since the first launch** — the app simply never asked, so each session looked like the
first and past work was unreachable, which for the researcher is the same as lost.

`POST /threads/search` lists them; `PATCH /threads/{id}` names them. The sidebar is now
220px with: newest-first list, click to reopen (transcript rebuilt from the thread's stored
messages, figures re-read off disk), a **New** button, and **rename in place** — the row
becomes a text field, the pattern chat apps use because it keeps the name next to the thing
being named.

**Auto-titling from the first prompt**, because a list of "New conversation" is a list of
nothing — the convention across ChatGPT, Claude and Codex, all of which also allow manual
rename, which is why both exist here
([thefrontkit](https://thefrontkit.com/blogs/ai-chat-ui-best-practices),
[codex#12564](https://github.com/openai/codex/issues/12564)).

Titles live in **thread metadata**, not a local file: the name belongs to the conversation,
survives a reinstall, and cannot drift out of sync with what it names.

### Files, grouped by what you would do with them

OUTPUTS now lists the thread's actual files above the agent-declared buckets, grouped
**Figures → Data → Documents → Other** — categories about use, not format. Read off disk
for the same reason plots are (§42): a file written by a script inside `execute` registers
no artifact, and those are most of them. Each row shows a size and opens in whatever the
researcher normally uses; figures first, because those are the ones you want to *look* at.

### Interactivity

Every row now responds to the pointer — `RAISED` on hover, `ACCENT_SOFT` for the selected
conversation, and the rename control appears only on the row under the cursor via
`group_hover`, so the list stays a list of names rather than a wall of controls.

### Still ahead

**Visible scrollbars** remain the highest-value single fix, and are still not done —
`overflow_y_scroll` draws nothing, so scrollable content reads as cut off (§40). Then a
`Button` component to end the copy-pasted eight-call button, bundled fonts, and SVG icons.

## 49. P6.7 continued: themes, panels, search, preview (2026-08-01)

Six requests. Researching Zed first collapsed three of them into one component.

### One pattern answers three requests

Zed has **over fifty picker modals**, all the same shape: a centred floating panel over a
dimmed workbench, list on one side, preview on the other, the editor still visible behind
([zed#59604](https://github.com/zed-industries/zed/pull/59604),
[Zed blog](https://zed.dev/blog/hidden-gems-part-1)). That shape is the answer to *view a
file in the middle*, *settings as a floating window*, and *fast search* — they are one
component with three contents, not three features.

The **file preview** is built on it: centred, dimmed backdrop, click-away to close,
figures rendered inline, Markdown through the existing renderer, everything else as text.
Reads a **bounded 400 lines** — the file most worth previewing is a big dataset, which is
exactly the one that would freeze the UI thread if read whole.

### Themes, adapted from Zed rather than copied

Zed ships theme *families* as JSON with semantic keys — `background`, `text`, `accent`,
`border`, `elevated_surface.background` — and loads more from extensions whose ids must end
in `-theme` ([Zed docs](https://zed.dev/docs/extensions/themes)). We took the shape and
dropped the registry: a researcher wants to pick a palette, not publish one.

- `theme::Theme` is that struct; colours are read through functions backed by **atomics**,
  so switching is a store the next frame sees — and the free rendering helpers, which have
  no `Context` to reach a GPUI global through, can still ask.
- **Four built-ins**: *Mini-Me Dark*, *Slate* (cool, blue accent, for people who do not
  want an orange app), *Paper* (light — the one case where a dark UI genuinely fails, on a
  projector), *High Contrast*.
- **Any JSON file in `themes/`** beside `settings.toml` is offered too, and a file named
  after a built-in *replaces* it — how someone tweaks the default instead of living with it.
  A malformed palette is logged and skipped, never fatal: a researcher locked out by their
  own theme file has no way back in to fix it.
- Clicking the picker **applies immediately**; cancelling Settings reverts. A palette is
  judged by looking at it, so a Save button between the click and the result is the wrong
  loop.

The contrast test now runs over **every** theme, so a palette added later cannot be added
unreadable. It failed twice while writing them — `Slate`'s error red at 4.35:1 and three of
`Paper`'s inks — and both shipped sets are values the build chose.

Writing `Paper` also exposed a rule worth stating: **elevation always raises luminance**.
The first attempt inverted it for light themes, which made "elevated" mean two different
things. A grey canvas with white cards gets the same rule everywhere.

### The rest

- **Conversation search** reuses the command palette's own fuzzy scorer, so `pap` finds
  *Rendimiento de papa*. Same behaviour as Zed's file finder, no new matching code.
- **Both panels toggle** from the status bar, and both toggles stay visible when their
  panel is closed — a collapsed panel with no way back is the commonest way this feature
  becomes a bug report.
- **A spinner while anything runs.** The first turn after launch spends 20–40 seconds
  building the agent, and a still window reads as a hang — the most common reason someone
  kills an app that was working. Four braille frames on GPUI's own animation: no font to
  ship, no SVG, reads as motion at any size.
- **The ◎ at top-left opens Settings**, as asked.

### Not done

Settings is still a right-hand pane rather than a modal — the picker component now exists,
so moving it is small, but it is a change to a pane that works and this batch was already
large. **Visible scrollbars** remain undone and remain the highest-value single fix.

## 50. P6.7 step two: three bugs, and Zed's gallery (2026-08-01)

Screenshots of Zed alongside the app, and six things wrong. Three were bugs.

### The bugs

**"Open outside" did nothing for any file.** `workspace::open` was written for folders and
began with `create_dir_all`, so on a *file* — which exists — it failed with `AlreadyExists`
and returned before Explorer was ever launched. It now only conjures a directory that is
missing. One line, and it had broken every file-open in the app.

**The conversation list was empty until the first question.** The backend spawned lazily on
the first turn, so at launch there was nothing to list and the app looked as though it had
never been used. It now warms up at startup, which pays twice: the sidebar has content
immediately, and the 20–40 second agent build happens while the researcher reads the window
instead of while they wait on an answer.

**Clicking the top-left mark reset the palette.** `open_settings` reloaded the whole draft
from disk *and* reapplied its theme, so opening the pane discarded the theme being looked
at. The live palette and the saved one are now separate: opening never touches the screen,
clicking a theme applies it, and **Close** — not open — puts the saved one back.

### Themes: a list, and Zed's whole gallery

The cycle button was wrong twice over — the only way to find a palette was to click through
every one, and there was no way to see what existed. Now every theme is listed with a
**five-dot swatch** of its background, panel, accent, text and error, so they can be
compared side by side. GPUI 0.2.2 has hover *styling* but no hover *event*, so a true live
preview needs a custom element; the swatch does the same job and shows all of them at once
rather than one at a time.

**Zed theme files load directly.** A Zed theme is JSON, so anything from
[zed.dev/extensions](https://zed.dev/extensions) can be dropped into `themes/` and used.
The importer maps the fifteen keys that mean something here out of the 142 in the published
schema (`zed.dev/schema/themes/v0.2.0.json`), falls back per field so a partial theme still
loads, and derives a hover shade Zed does not define.

What we **cannot** use is a Zed *extension*: those are WASM compiled against Zed's own
extension API, and running one would mean implementing Zed. The theme JSON inside them is
portable, and that is the part worth having — a distinction worth stating plainly rather
than promising "install Zed extensions" and shipping something that does not.

### Rainbow CSV, from the theme rather than a fixed rainbow

CSV cells are coloured by column index — the `rainbow-csv` trick — but cycling **the
palette's own roles** instead of a fixed spectrum. Those colours are already contrast-checked
against every surface, so a wide table stays readable in the light theme too, where a fixed
rainbow would wash out. Without column *layout*, which GPUI 0.2.2 lacks, colour is the only
thing that makes a wide CSV readable at all.

### Rounded panels

Panels are now rounded cards with a margin, sitting on the window background, rather than
full-bleed slabs meeting at a hairline — which is what the Zed screenshots show and most of
why they look finished.

### Still not done

Settings remains a right-hand pane rather than a centred modal, and **visible scrollbars**
remain undone and remain the highest-value single fix.

## 51. Thirty conversations that were not conversations (2026-08-01)

The sidebar worked, and immediately showed what was wrong with it: **thirty rows reading
"New conversation"**, above the two real ones.

They were not conversations. Every background worker creates a **thread of its own** (§43) —
that is what makes it a background worker — and `POST /threads/search` returns every thread
the backend has. The list was machinery, correctly listed.

Fixed by tagging what this app creates: `POST /threads` now writes
`metadata.minime_conversation`, and the search filters on it. The distinguishing fact is
**who created the thread**, and only this app tags what it creates — so the filter is exact,
where "has messages" or "has a title" would be guesses that keep being wrong. A background
worker's thread is real and deliberately is not one of these.

**Threads created before this change will not appear.** They are almost entirely the
"New conversation" rows; the two real ones are lost from the list, which is the cheaper
mistake than a list nobody trusts.

### Also from the same screenshot

- **The status bar was clipped off the bottom edge.** It is the last child of a column whose
  transcript grows, and a flex child shrinks by default — the third time `flex_none` has
  been the answer (§40, §48).
- **The empty state named a Run button** that has not existed since the composer landed
  (§12) — the first sentence a new user reads, describing a UI from weeks ago.
- **Settings became a centred modal.** As a column it took 420px off the chat for as long
  as it was open; it is somewhere you visit and leave, which is the same argument that
  makes Zed's fifty pickers modal rather than panels. This closes the last of the six
  requests from §49.
- The search field now looks like a field: rounded, its own background, a border.

## 52. Scrollbars, a real theme gallery, and a correction (2026-08-01)

### The correction first

I told the researcher that Zed extensions are WASM compiled against Zed's own API and so
could not be used here. That is true of **language** extensions and false of **theme**
extensions, and the registry says so plainly:

```json
{"id":"catppuccin","provides":["themes"],"wasm_api_version":null,"download_count":964535}
```

`wasm_api_version: null` — pure data, a `.tar.gz` of JSON. I generalised from the kind of
extension I had read about to all of them, and made a feasible feature sound impossible.
The lesson is the one from §35 and §39 in a new costume: **check the thing itself before
asserting what it is.** Two public endpoints and one `curl` would have settled it.

I also said the live theme preview needed a custom GPUI element. `InteractiveElement`
has **`on_hover`**. Same mistake, same size, found the same way — by looking.

### The gallery

`gallery.rs` searches `api.zed.dev/extensions`, keeps only entries whose `provides`
contains `themes`, sorts by installs, downloads the archive and writes `themes/*.json` into
the same directory a researcher can drop files into by hand. Author and install count are
shown because these are other people's work under their own licences.

It reads **only** `themes/*.json` and builds the write path from the *file name*, never the
archive's own path — the classic tar traversal, worth ruling out even from a registry we
trust. Everything else in the archive (licence, manifest, readme) is not ours to interpret.

It is a **theme** installer, not an extension installer, and the module says so: promising
the latter and shipping the former would be worse than naming which one this is.

### Scrollbars, at last

`overflow_y_scroll` draws nothing, which is why the approval card read as broken (§40) and
why nobody found Save below the fold in Settings. `ScrollHandle` exposes `offset()`,
`max_offset()` and `bounds()` — enough to size and place a thumb, with no custom element.
It sits *outside* the scrolling div, in a `relative()` wrapper; inside, it would scroll
along with the thing it measures.

### The send button

Researched rather than invented. Shipped composers converge on three states, and the
button now has all three: a filled circular control that **sends**, the same control greyed
when the field is empty — *empty means disabled* is near-universal — and a **stop** while a
turn streams. Clicking stop currently says cancelling is not built yet, which is at least
honest; the control is where it will live.

The composer and its button are now one bordered, rounded field rather than a text box next
to an unrelated button.

### Settings, again

The modal scrolled *as a whole*, so **Save and Close were below the fold** — the same
defect as the approval card in §40, in the component built to fix that class of problem.
Title and actions are now fixed and only the middle scrolls. Third occurrence; the lesson
is evidently not learned by writing it down, which is an argument for the `Button` and
`Modal` components P6.7 still owes.

## 53. The status bar belongs to the window, not the chat (2026-08-01)

Catppuccin installed from the gallery and applied — the §52 path works end to end. One
thing left over from the layout:

The status bar lived *inside* the chat pane, so it was only as wide as the chat, and its
controls slid left and right every time a panel was collapsed. **A status bar that moves is
one you have to look for**, which defeats the point of putting the panel toggles there.

Zed's runs the full width of the window for exactly this reason. The layout is now a column
— panels in a row on top, status bar spanning the window beneath — so the toggles, the
execution indicator and the URL stay where they are whatever is open.

`min_h_0` on the row, for the fourth time (§40, §48, §51): a flex child refuses to shrink
below its content, so without it the row pushed the status bar off the bottom edge. Four
bugs from one default is a strong argument that the layout primitives should be wrapped
once, correctly, rather than re-specified at every call site — which is the `Button` and
`Modal` work P6.7 still owes.

## 54. P6.7 closed (2026-08-02)

Verified on Windows: Catppuccin installed from Zed's gallery inside the app, panels
collapse without the status bar's controls moving, the composer reads as one field with a
live send control, and the sidebar remembers conversations across launches.

**Every milestone from P6.0 to P6.7 is now closed.** What the app does is done; what
remains is getting it to a second person, and paying down the debt underneath the UI.

### The debt, stated plainly, because it is now the top item

Two mistakes have each been made three or four times:

| Mistake | Where | Cost |
|---|---|---|
| A flex child shrinking or growing when it must not (`flex_none`, `min_h_0`) | §40, §48, §51, §53 | Four bugs |
| Actions placed *inside* a scrolling region | §40, §41, §52 | Three bugs |

The third and fourth occurrences happened **after** the lesson was written down, and one of
them was inside the component built to fix the first. Writing it down is evidently not the
mechanism. A wrapped `Button`, `Modal` and `Panel` — where the scroll region and the action
row cannot be mis-nested because the type does not allow it — is the mechanism, and it is
now worth more than any new feature.

### What was learned about researching before building

Three times this batch, checking the artefact beat reasoning about it:

- `on_hover` **exists** in GPUI 0.2.2 — I had said a live theme preview needed a custom
  element (§52).
- Zed **theme** extensions are pure data (`wasm_api_version: null`), not WASM — I had said
  the gallery was unusable (§52).
- `ScrollHandle` exposes `bounds()` and `max_offset()`, which is all a scrollbar needs — I
  had listed scrollbars as a large piece for weeks.

All three were one `grep` or one `curl` away, and all three had been asserted confidently
in the opposite direction. Same shape as §35's seven-round theorizer failure and §39's
`TypeError`: **the expensive errors in this project have all been things assumed rather
than checked.**

## 55. Multi-line prompts, and a design for naming a subagent (2026-08-02)

### The composer is a real field now

`Enter` still sends — rebinding that in a chat window would surprise everyone — and
**Shift-Enter** inserts a line break. That required making the element genuinely
multi-line, not just accepting a `\n` the renderer swallows:

- one `ShapedLine` per line, since GPUI's `shape_line` is exactly that, one line;
- height follows the content, capped at **8 lines**, so a pasted script enlarges the
  composer instead of eating the transcript;
- the caret finds its row, and a selection spanning lines paints **one rectangle per line**
  — a single box from start to end would paint over the text in between;
- hit-testing maps a click to a row and then along it;
- IME runs are **sliced per line**, or marked text would underline the wrong characters.

### P7 proposal — `/subagent` commands

The request: `/eda-subagent make an EDA of data.csv`, `/research-paper search this topic`,
`/report-write write a report from these papers`. Today the coordinator decides what to
delegate; this would let the researcher say it outright.

**What makes this cheap:** the machinery already exists. `start_async_task(description,
subagent_type)` takes the subagent by name, this app already wraps that tool (§38), and the
Jobs panel already tracks, approves and reports whatever it starts (§31, §42). A slash
command is a *front end* to a call we already make.

**Shape:**

1. **A registry of what can be named**, read from the backend rather than hardcoded — the
   coordinator's own subagent list is already in the graph, so a hardcoded list here would
   drift the first time upstream renames one.
2. **Completion in the composer.** Typing `/` opens the same fuzzy picker the command
   palette and conversation search use (`match_score`), showing each subagent's
   description. Nothing new to build but the trigger.
3. **Two dispatch modes**, and the difference matters:
   - **Foreground** — the turn runs as now, with the prompt prefixed to name the subagent.
     Right for a quick literature lookup.
   - **Background** — straight to `start_async_task` with `subagent_type` set, returning a
     task id immediately. Right for an EDA or a report, and the reason this is worth
     building: it is the only way to run three of them at once.
4. **The gate still applies.** A named subagent is still an agent running commands on the
   researcher's machine, and `/eda-subagent` must not become a way around the approval
   gate (§19, §41).

**The open question** is what happens when a named subagent does not exist — a typo, or an
upstream rename. Failing loudly at send is right; silently sending `/eda-subagent …` as
prose is how someone waits ten minutes for a turn that was never delegated.

Sequenced after the release link and the component work, because it is an accelerator for
people already using the app, and nobody outside this machine can install it yet.

## 56. The build no longer needs anyone's laptop (2026-08-02)

`release` #1, manually run, **green in 21m33s** on the first attempt — which is not what I
expected, having written the workflow without being able to run it.

The two steps I flagged as most likely to fail both passed:

- **`fxc.exe`** — found in the Windows SDK on the runner and exported as `GPUI_FXC_PATH`,
  so GPUI compiled its release shaders (§26).
- **The private backend** — cloned with the `MINIME_BACKEND_TOKEN` secret, so the bundle
  can install itself without the researcher having a GitHub account (§25).

`workflow_dispatch` was the right default: the run produced a downloadable artifact and
created no release, so the pipeline could be proved before anything public existed.

**What this changes.** A release stops depending on one person's Windows machine being
free, and stops depending on the state of that machine — the packaged zip is now built from
a clean checkout every time, which is the only way "it works on mine" ever gets ruled out.
`scripts/release.sh` remains as the local path and as the thing that documented what the
pipeline had to do.

**Still not proven:** nobody has installed this zip on a machine that did not build it.
That is the next step and the one that matters — the first-run WSL path has never executed
anywhere.

## 57. The first clean machine, and the blindness it exposed (2026-08-02)

A colleague ran the CI build on a laptop that had never had this app. **Most of it worked
on the first try** — the window rendered, the Setup pane opened, the checks ran, and it
correctly diagnosed *"WSL is present but no distro answered"* and offered the fix.

Then the fix failed, and reported:

```
Install Ubuntu — failed
— the command reported a failure — the last lines say why
```

**There were no lines.** The box was empty.

### Two defects, and the second is worse than the first

**`wsl --install` needs administrator rights.** The app is not elevated and must not be, and
a process cannot elevate itself — so the command failed instantly. The check's own note even
said *"asks for admin rights"*, and nothing acted on it. Fixed by wrapping it in
`Start-Process -Verb RunAs`, which is Windows asking the researcher rather than us pretending
we can. `-Wait` so "finished" means finished, and `exit $p.ExitCode` so a **refused** UAC
prompt reports failure instead of success.

**The log was empty because we could not read it.** This line:

```rust
for line in BufReader::new(pipe).lines().map_while(Result::ok)
```

`lines()` yields an error on the first byte that is not UTF-8, and `map_while` **stops at the
first error**. `wsl.exe` writes **UTF-16LE** — a fact already recorded in this project's own
notes about `wsl -l` — so every line was an error and the iterator ended before yielding
anything. Every fix that failed with a UTF-16 message has been reporting *nothing* since
`run_streaming` was written.

Now the pipe is read as **bytes**, the encoding is detected once from the first chunk (BOM,
or ASCII padded with NULs), and lines are split on `0A 00` or `0A` accordingly — cutting a
UTF-16 stream on a bare `0A` would leave the stray `00` at the head of the next line and
shift every character after it. A trailing line with no newline is kept, because that is
often the only line a failing command produces.

### The pattern, for the fourth time

*"The last lines say why"* over an empty box is exactly §35's theorizer reporting a guess,
§38's *"The async subagent encountered an error"*, and §39's silent `TypeError`. **Four
times this project has told someone a reason exists while showing them nothing.** The note
now says only *"the command reported a failure"*, and the card says plainly *"The command
printed nothing"* when that is the truth.

The rule, stated as a rule this time: **never write a message that promises output unless
the code path that produces it has been seen to produce output.**

### What this run proved

The packaging, the CI build, the unsigned-binary path past SmartScreen, the window, the
preflight checks and the diagnosis all work on a machine that did not build them. The
remaining unknown is now narrow: whether the elevated install actually completes.

## 58. Six things that only appear once the app is used (2026-08-02)

Catppuccin installed and applied, and with eleven palettes in the list the design started
failing in ways four never could.

- **The theme list is scrollable and filterable.** Eleven entries already crowded the modal;
  a hundred would push Save out of it. Capped at 260px with its own scrollbar, and a filter
  using the same fuzzy scorer as everywhere else, so `mocha` finds *Catppuccin Mocha*. The
  cure for a long list is a filter, not a taller list.
- **The model is a scrollable list, not a remembered string.** Nobody should have to know
  whether it is `claude-sonnet-4-5` or `claude-4.5-sonnet`. A short curated set per
  provider, and **the field stays editable** — a list here can only ever be out of date, and
  a provider shipping a model the day after a release must not make the app unusable.
- **Providers are pills, not a cycle button.** Five fit on one row, and a cycle button hides
  four of the five — the same complaint the theme list had just answered.
- **Escape closes things.** It was bound *only* inside the palette's key context, and focus
  is almost always in the composer, so it never reached a modal. Now bound with **no**
  context, closing inside-out: preview, then a rename in progress, then Settings, then
  Setup. From a preview over Settings it returns to Settings, not to nothing.
- **Conversations can be deleted**, in two steps. There is no undo on the server and a
  conversation is somebody's work, so a stray click on a `✕` in a list must not destroy it.
  Files the turn wrote are deliberately **not** removed: those live in the researcher's own
  Documents and are not ours to delete.
- **Square corners are gone** — twelve buttons, the bordered inputs and the fix log now
  match the panels rounded in §50.

### The debt this keeps proving

Every one of these was a call-site mistake: a list without a cap, a control without
rounding, a binding with the wrong scope. None was a hard problem, and all of them were
invisible until someone used the app with real data in it. That is the fifth argument in this
document for the `Button` / `Modal` / `Panel` set — a `Button` cannot be square, and a
`Modal` body cannot grow past its frame, if the type is the only way to make one.

## 59. Every model rendered as "…" (2026-08-02)

The model list shipped in §58 showed five rows, each reading exactly `…`.

`.truncate()` was on the **row** — the flex item itself — together with `min_w_0`. That
combination gives the element zero intrinsic width, so there is nothing to truncate *to* and
the ellipsis is all that survives. Every other truncating label in this file happens to sit
in an inner div with `flex_grow().min_w_0()`, which is why none of them showed this and why I
did not notice writing it a sixth time.

Fixed by matching that pattern: the row lays out, the **label** truncates.

Two smaller things the same screenshot showed:

- The selected row had a border and its neighbours did not, so it was a pixel taller and the
  list **jumped** as the pointer moved down it. Selection is now background and colour only,
  with a `✓` — no geometry change.
- The free-text field beneath the list was labelled *"Model"*, which read as a second control
  contradicting the first. It now says **"Or type any model id"**, which is what it is for:
  the list is curated and will go stale, and typing has to keep working.

This is the sixth call-site mistake in two days, and the second where the correct pattern was
already in the same file. Not a knowledge problem — a repetition problem.

## 60. What a screenshot of a blank console proved (2026-08-04)

A machine that had WSL but no distro clicked **Install Ubuntu**. Two screenshots came back.

The first showed a real elevated `wsl.exe` console downloading *"Subsistema de Windows para
Linux 2.7.11"* — so §57's elevation fix works. The second, taken after it finished, showed the
Setup pane saying **"Install Ubuntu — done"** with a body containing exactly one line, `—
finished`, directly above the same red **✗ WSL2 runtime** row it had started from.

Three separate defects, and the first one is the interesting one.

**The output was never ours to read.** `-Verb RunAs` elevates through ShellExecute, which
cannot be handed the pipes we opened: the elevated child gets a console of its own. That is
the window in the second screenshot. We captured the PowerShell wrapper, which prints nothing.
§57 fixed the *encoding* of output we were never receiving — a correct fix to the wrong
layer, and it looked verified because the code path it repaired was real.

So the elevated command now runs through `cmd.exe` purely to get `> "<log>" 2>&1`, into a path
in the user's own temp directory, and `run_streaming` **follows that file while it grows**
rather than draining it at the end — `wsl --install` downloads for minutes, and a pane reading
"starting…" for four of them is a pane a researcher reads as stuck. A terminated line goes out
immediately; only an unterminated tail waits, because it may be half-written.

**"Done" over a red row.** The install exited 0 having replaced the WSL runtime, and Windows
will not register a distro until it restarts. The app had no way to notice, so it congratulated
itself and contradicted itself in the same card. Now a fix that succeeds is compared against
the row it was meant to fix, and when that row is still failing the card says so — for the
runtime row, that Windows has to restart first.

That verdict is drawn from the re-check, **never from the command's own words**. This machine
printed *Descargando*, not *Downloading*: `wsl.exe` speaks the system language, so matching
output for "restart" would have failed for precisely the user who needed it. Roughly 98% of
this app's users are on Windows and not all of them are on an English one.

**A dash where an admission should have been.** `— finished` was pushed into the same `Vec` as
the command's own output, so the "this command printed nothing" message could never fire — the
note itself made the list non-empty. Notes now live apart from output and render *outside* the
scrolling box, since the verdict and the next step are the two things a chatty command must not
be able to scroll out of sight.

**One hazard the fix introduced, caught before shipping.** Redirecting stdout would have made
`wsl --install -d Ubuntu`'s interactive prompt for a UNIX username *invisible* — a blank window
waiting forever on input. It now passes `--no-launch`. A researcher who cannot code should not
have to invent a Linux password to read a paper; unlaunched, the distro answers as root, which
is all the sidecar needs.

Two smaller notes:

- Writing the live-progress test is what found the hold-back bug: every complete line was
  waiting for the *next* one, which would have shown `wsl.exe`'s progress permanently one step
  behind. The test failed for the right reason before the feature was wrong in front of a user.
- The first two attempts at that test failed by racing each other over `elevated_log()`, a
  single fixed path. That path is correct — the app runs one fix at a time — so they are now
  one test, which is also the honest shape: three facts about one mechanism.

And once more for the record: I claimed `theme::danger()` existed on the strength of a grep
that was matching **my own uncommitted edit**. The compiler caught it in seconds. That is the
fifth confident claim about this artefact to dissolve on contact (§52 has three, the
text-selection claim has one) — and the first where the false evidence was something I had just
written myself.

## 61. A machine that had no WSL, working (2026-08-05)

A third laptop, and the answer the last three sections were waiting for. Elevated install,
restart, reopen — **4 ok · 1 to fix · 1 optional**:

- ✓ WSL2 runtime — *a distro started and answered*
- ✓ Mini-Me backend — *langgraph.json found in ~/.local/share/mini-me-desktop/backend*
- ✓ Python dependencies — *.venv/bin/langgraph is installed*
- ✓ Host execution overlay — *installed with the backend*
- ! Asta CLI — signed out, and mid-device-code as the screenshot was taken
- ✗ Model API key — the one thing left, and it is a paste

Nobody typed a command. The app installed WSL, provisioned the checkout, ran `uv sync`, put the
overlay in place, and then diagnosed the two things only a person can supply. **This closes the
item that has been top of "blocks a second person using it" since §45** — a first run on a
machine that had never had any of this now works, and the remaining two rows are a sign-in and
a key.

The restart is the part worth remembering: on both machines that lacked WSL, `wsl --install`
replaced the WSL runtime and Windows would not register a distro until it rebooted. §60 taught
the app to say so. This laptop confirms that once it does reboot, everything downstream lands.

### The correction: `--no-launch` was a mistake, and it was mine

§60 added `--no-launch` to the install, reasoning that the post-install launch asks for a UNIX
username and that with output redirected the question would be invisible. The reasoning about
the hazard was right. The flag was wrong.

`--no-launch` is documented, and it also has a documented failure: it can install the distro
**without registering it** under `HKCU\Software\Microsoft\Windows\CurrentVersion\Lxss`, so
`wsl -l -v` does not list it and the only cure is to run the install again *without* the flag
([microsoft/WSL#10646](https://github.com/microsoft/WSL/issues/10646)). Our runtime probe asks a
distro to answer. An unregistered distro does not answer. The flag could therefore have
manufactured the exact state the button exists to escape — on the very path this laptop had
just proven.

Reverted. The hazard is handled where it belongs, by denying stdin: `< NUL` on the elevated
command. At EOF the username prompt gives up instead of waiting forever, the install and
registration follow the ordinary proven path, and the distro answers as root — which is all the
sidecar needs. An elevated fix can never be interactive anyway; its console is not one we can
put a question in.

Two things about how this was caught. It was caught by **research, before shipping**, because
this project's standing rule is that research is mandatory and one `WebFetch` of the WSL docs
and issue tracker settled it. And it was a change made to a command that had *just been proven
to work* — speculative hardening on a proven path, which is a worse trade than the hazard it
was guarding against. §60 counted five confident claims that dissolved on contact; this is the
sixth, and the first one I found before a user did.

## 62. Text selection, the thing this document said was impossible (2026-08-05)

For two months the plan recorded that selecting text in the transcript "is the one thing here
the framework genuinely makes hard — GPUI 0.2.2 cannot." §59's follow-up already corrected the
claim; this is the feature.

`gpui::TextLayout` exposes `index_for_position` (`elements/text.rs:483`) and its inverse
`position_for_index` (`:517`) — a hit-test and a caret position, which between them are
everything selection needs. What GPUI genuinely does *not* provide is selection **state and
painting**: nothing under `gpui/src/elements/` mentions the word, and `InteractiveText` offers
click and hover indices only. So the missing half is `crates/app/src/selection.rs`, and the
original claim was two thirds wrong.

**Not one big element.** The transcript is a tree of divs — headings, paragraphs, list items,
table cells, code blocks — each holding a `StyledText`. Replacing that with a single custom
element would have meant re-implementing Markdown layout, so instead each run of text is
wrapped in a `Selectable` that delegates layout and paint to the `StyledText` it holds and adds
exactly two things: it registers its `TextLayout` in a shared registry under an index assigned
in document order, and it paints the part of the selection that falls inside it. Bold, links,
inline code and rainbow CSV columns all still work *because nothing about them changed*.

Selection quads are painted **before** the glyphs. Painted after, the highlight covers the words
it is meant to highlight.

The registry is rebuilt every frame. Layouts move when the window resizes, when the transcript
scrolls and on every streamed token, and a rectangle from the last frame is a highlight over the
wrong words. The *selection* survives, because it belongs to the user — except when the
transcript is emptied, where it is cleared at all three sites: span indices are positions in one
conversation, and keeping one across a thread switch would highlight whatever text happened to
land in the same place.

**Copy, without stealing a key.** `ctrl-c` is bound with no key context, the same reasoning as
Escape in §58: focus lives in the composer almost always, so a workbench-scoped binding would
never be reached. The composer's own `ctrl-c` is more specific and still wins — it just calls
`cx.propagate()` when it has nothing selected, handing the shortcut down. So copying out of the
transcript needs no click to move focus first, and copying out of the composer is unchanged.
"Select everything" is `ctrl-shift-a`, not `ctrl-a`: that one belongs to the composer, where it
selects the prompt being typed. Both are also in the command palette, because a reader who has
never met this app will not guess either.

### What is tested, and what a person still has to look at

Twelve tests. The selection algebra — a click with no drag selects nothing, dragging upwards
selects the same text as dragging down, a span passed straight through is covered end to end, an
offset past the end of a span is clamped rather than panicking, a new frame keeps the selection
and drops the rectangles.

And the geometry, which is the part that cannot be eyeballed on a headless machine: `rows_between`
was split out of the painting so it could be tested with plain numbers — one line is one
rectangle between the two points, a wrapped selection runs to the edge on every line but the
last, a selection ending exactly at a line start draws no sliver, and a zero line height does not
loop forever. Counted rows rather than `y += line_height` until it passes `to.y`: that compares
f32s against a bound it can overshoot, and a selection that drops or doubles its last line is a
bug that only appears on the one paragraph that wraps.

Three assumptions were checked against the pinned crate rather than assumed, since the whole
premise of this section is a claim that was not: `StyledText::prepaint` does set
`element_state.bounds` (so `TextLayout::bounds()`, which unwraps twice, cannot panic on a span
registered after its inner prepaint); `chat_pane` is called exactly once per render (so span
indices are handed out in document order and cannot collide); and `cx.propagate()` exists
(`app.rs:1720`), which is what the `ctrl-c` handoff rests on.

**Unverified: everything a person sees.** This machine has no display, so no one has yet dragged
across a paragraph and watched the highlight appear. The algebra and the geometry are tested, the
panics are reasoned out against the crate source, and the failure modes left are visual — a tint
too faint, a highlight a pixel off a descender, a drag that feels wrong at the edges of a table
cell. Those need the researcher's eyes, and saying so is more use than claiming they are done.

## 63. A stop button that stops something (2026-08-05)

Since §52 the stop button has been honest and useless: it said *"cancelling a running turn is
not built yet"* and meant it. What it needed was a name for the thing to stop.

**The run id was arriving all along and being thrown away.** The first frame of every stream is
`event: metadata`, and `tests/fixtures/delegated-turn.sse` — captured from a real turn — shows
its payload is `{"run_id":"019fb670-…","attempt":1}`. The decoder mapped that frame to
`Status("run started")` and dropped the rest. It now also emits `TurnEvent::Started { run_id }`,
which the sidecar notes on the way past.

**Aborting our own stream is not cancelling.** This is the part that would have made a
half-finished feature look finished. Dropping the SSE response closes the connection, and
LangGraph's `on_disconnect` defaults to `continue` — so the graph keeps running, and an agent
that spends tokens per step keeps spending them, with nobody reading the answer. Stop therefore
does both: `POST /threads/{id}/runs/{id}/cancel` and then aborts the local task.

Both facts came from the SDK vendored in the reference checkout rather than from memory:
`DisconnectMode = "cancel" | "continue"` and `cancel(threadId, runId, wait?, action?)`, whose
default action is `interrupt`. `interrupt` is the right one here — `rollback` would erase the
partial answer the reader is looking at, which is real work and quite possibly the reason they
pressed stop.

Three smaller decisions, each about not lying:

- **The partial answer stays**, and is *marked*: "you stopped this turn; the answer above is
  incomplete". A truncated answer and a complete one are otherwise identical on screen, and the
  difference decides whether the thing can be relied on. A stopped message with no body at all
  also stops counting as silent, so the pruner cannot delete the only record that it happened.
- **Resumed continuations are cancellable too.** An approved command is often the slowest part
  of a run, which is exactly when someone reaches for stop.
- **The window where there is no id** — the few milliseconds before the first frame — reports
  *"stopped watching — the run had not reported an id yet, so the backend may still be
  finishing it"*, rather than "turn stopped". Two different things happened; they get two
  different sentences.

## 64. Right-click, and a paste that had been broken since §55 (2026-08-05)

§62 gave the transcript something worth copying and then asked the reader to know that `ctrl-c`
is how. A right-click is what everyone tries first.

GPUI ships no menu widget, the same way it ships no text input. It does ship `anchored`, which
places a child at a point in the window and keeps it inside the frame, and `deferred`, which
paints it after everything else so it is not clipped by the pane it opened over. Those two are
the whole mechanism.

No menu item decides what it does — each names a method that already exists and is already on a
key, so the menu is a second door onto the same room rather than a second implementation. Cut
and Paste are **absent** from the transcript rather than greyed, because it is not editable and
never will be; inside the composer they are always present and greyed when unavailable, because
there they are only temporarily out of reach. Every row shows its own binding, and Select all
shows a *different* one per target, because it genuinely is a different key.

Escape closes the menu before anything else, innermost-first per §58. A right-click elsewhere is
handled only by the opener: closing on click-out as well would race it, and which won would
depend on paint order — sometimes leaving no menu at all.

**And the thing this turned up.** Wiring Paste meant reading the composer's paste handler, which
still said *"Newlines would break single-line layout; flatten them"* and replaced every `\n`
with a space. That comment was true when it was written and stopped being true in §55, which
made the composer genuinely multi-line — for the express purpose of accepting "a prompt carrying
a script, a table or a list". So for two months pasting exactly that silently ran it all
together, and the module header still described the field as "single-line by design".

Both fixed. `\r\n` is normalised on the way in, because a Windows clipboard is full of it and a
stray `\r` shapes as a box.

Worth naming as a category: this is the third defect found by *reading code adjacent to the one
being changed* rather than by testing the change — §60's empty-log message and §59's
already-correct-pattern-in-the-same-file being the others. A stale comment describing behaviour
that has since changed is not a documentation problem; it is a bug with a note attached.

## 65. Blockquotes, nested lists, and an image that was showing its punctuation (2026-08-05)

The last of the Markdown gaps. All three are what a coordinator writing a report actually emits,
and until now all three rendered as source.

**Nested lists are depth from a stack, not from arithmetic.** The obvious implementation is
`indent / 2`, and it is wrong: agents write two-space *and* four-space nesting, sometimes in the
same answer, so a fixed divisor renders one of them flat and the other twice as deep as written.
Instead the parser keeps the indents it has actually seen in the current list, shallowest first;
its length is the depth. Deeper than the top opens a level, shallower closes as many as it must.
Two-space and four-space sources therefore produce identical trees, and no level can be invented
that the source did not indent for. A tab counts as four columns, because counted as one it
would sort a tab-indented child *above* a two-space one.

A numbered item keeps the number its author wrote at any depth. Renumbering it — or swapping in
a bullet because it happens to be indented — would change what the answer says, and steps get
referred to by number.

**Blockquotes** fold consecutive quoted lines into one block and count their `>` markers, so a
quoted quote still reads as one. Emphasis inside is parsed normally. Rendered as a rule down the
left in muted text, because a quote is something the answer is *referring* to.

**The image was leaking its own syntax.** `![alt](url)` rendered as `!alt url`: the inline scan
stopped at `[`, so the bang was pushed as ordinary prose before the link branch ever saw it.
Adding `!` to the scan set is what fixes it — which then puts every exclamation mark in ordinary
writing through that branch, hence a test over `Done! Next?`, `Careful! [see here](x)`, `a ! b`
and a bare `!`.

An image alone on a line becomes a block; inside a sentence it stays inline, because promoting
it would tear the sentence in half.

**What the image block deliberately does not do is load the picture.** The path points into the
distro's filesystem, and translating it into something Windows can open does not exist in that
direction — §46 recorded the three spellings a path has here, and this is the missing fourth.
Meanwhile figures the agent really produced already appear beneath the answer, found by diffing
the thread's output directory *on the host* (§42). So the block names the file and stops. Showing
a broken image, or a path the reader cannot open, would be worse than a caption.

Eleven tests, which is the point of having done this one now: every claim above is checkable
without a window, unlike the UI debt sitting above it in the list.

### Why the component set is still not next

`Button`/`Label`/`Modal`/`Panel` has been top of the UI-debt list since §58, and it stays there
for now on a straight risk trade. It is a sweeping restyle of every control in a 6,000-line file,
its whole value is preventing a *visual* class of bug, and it cannot be verified from a machine
with no display — right after the app first worked end to end on someone else's laptop. The six
call-site mistakes it would prevent were each caught within a day. Blocking a release-adjacent
week on a blind refactor to prevent a bug class that is currently being caught cheaply is the
wrong order. It goes next to a window someone can look at.

## 66. The nested quote that had already vanished (2026-08-05)

§65 shipped blockquotes an hour before a real answer was asked to render one. The answer
contained `> First quoted line` followed by `>> Nested second-level quote`, and both lines came
out **run together on a single depth-1 rule**. The nesting was gone.

The fold collected every consecutive quoted line but kept only the *first* line's depth. So a
`>>` after a `>` was appended to the outer quote's text and its own depth thrown away — while
`Block::Quote` carried a `depth` field, and a test asserted `>>` alone produced depth 2. The
field worked; the fold never gave it the chance.

Now only lines at the *same* depth keep folding, with a test for both halves: a quote inside a
quote is its own block, and an ordinary two-line quote is still one block rather than two rules
stacked on each other.

Worth being precise about what the eleven tests in §65 did and did not buy. They caught nothing
here, because every one of them tested a single construct in isolation — `>>` on its own line
passed, `>` twice passed. The defect lived in the *transition* between two constructs, which is
exactly the shape unit tests miss and one real answer found immediately. Everything else in that
answer rendered correctly, including the thing the section was built for: two-space and
four-space nesting produced identical depth.

## 67. A button you cannot build wrong (2026-08-05)

The component set, finally, and starting with the half that could be verified from a headless
machine.

**What the survey said, rather than what I would have guessed.** Before writing anything, the
44 clickables in `main.rs` were counted by what they actually do: borders cluster into exactly
three — 36 `border()`, 12 `accent()`, 1 `error()`. Sizes into two — `px_3 py_1 text_sm` and a
compact `text_xs`. So `Tone` has two variants and `Size` has three, and every colour, padding
and radius in `ui.rs` was read out of the call sites it replaces. Migrating a button is meant to
change nothing on screen.

**Except that it changed twelve of them, which was the point.** §58 rounded the corners of the
app and missed **eleven bordered buttons**; a tighter scan while migrating found a twelfth,
`setup-to-settings` — the *Settings* button sitting immediately beside the already-rounded
*Re-check* in the pane every new user meets. `Install Ubuntu` and `Copy ⧉`, in every Setup
screenshot in this document, were square for two months. They are not any more, and not because
anyone remembered: `rounded_md` is not reachable to omit.

`Button` also makes `disabled` one flag instead of a colour plus a guard. Those were kept in
step by hand at each site, and the click listener is now attached *only* when the button is
live — so "disabled" cannot be true in the styling and false in the behaviour.

`Label::ellipsis()` is the §59 lesson as a type. `.truncate()` on a flex item with `min_w_0`
leaves zero intrinsic width and renders as nothing but an ellipsis; `ellipsis()` always pairs it
with `flex_grow`, and there is no way to ask for the broken combination. Six call sites, all of
which already had it right — the value is that the seventh cannot get it wrong.

### Three controls deliberately left hand-written

Each now carries a comment saying why, so the next person does not copy one as the template:

- **the provider pill** — has a *chosen* state with its own background; "which of these is
  picked" is a different control from "press this to do a thing";
- **the settings toggle** — a full-width checkbox row, where every `Button` is `flex_none`;
- **New** in the sidebar — brightens border *and* text on hover, which nothing else does. One
  call site is not worth a flag on a shared type.

### What is not here, and why

No `Modal`, no `Panel`. Both are about where actions sit relative to a scroll area — the other
repeated bug, three of them — and that means migrating thirteen scrolling panes whose only proof
is visual. `scrolling()` was written, found to have no honest caller in this pass, and deleted
rather than shipped unused. Same for a `Danger` tone: the one destructive control in the app is
a borderless chip inside an already-red confirmation row, so it is one of the twenty-three rows
this type deliberately does not cover, and a tone with no caller is a design vocabulary invented
ahead of the design.

That is the next increment, and it wants a window open.

## 68. Setup stops being a pane and becomes a page (2026-08-05)

Four screenshots of Zed's settings window settled a question this app had been answering badly:
**where does configuration live?** Zed's answer is one floating window with a nav rail down the
left, a page on the right, and every row shaped the same — title and description on one side,
the control that changes it on the other.

Ours had Settings as a centred modal and Setup as a 420px column in the right-hand slot. That
slot belongs to the research panel, so **opening Setup hid the outputs you were diagnosing it
about**, and Setup's own "Settings" button existed only to get from one to the other.

Both are now pages of one window: Appearance, Model, Research, Backend, Setup. Reaching Setup is
a click in the rail, and the button that used to shuttle between them is gone. The first blocked
report still opens the window by itself, on the page that says what is wrong — the guided first
run, unchanged in behaviour and no longer costing the chat 420px.

**`ui::Modal` is what made this cheap, and it is a shape rather than a style.** Actions inside
the scroll area is three separate bugs (§40, §41, §52) and the fix each time was moving one
`div` out of one container. Here `body` and `actions` are separate slots, so a Save button has
nowhere to scroll away to; `min_h_0` on the body is inside the component, so a long page cannot
push the buttons off the bottom (§40, §48, §51, §53). Both defects are now unreachable rather
than remembered.

Read out of `settings_ui.rs` rather than guessed from the pictures: Zed builds the rail from a
two-level `NavBarEntry` tree, and a row is `h_flex().justify_between()` with a title-and-
description stack against the control. The rail here is flat, because two levels is one more
than this app has sections for.

**The toggles gained a sentence each, and that is the real change.** They were `☑ Run code on
this machine` — one element that read the same way, which was the argument for them in the first
place and was true. But it made a setting's *name* and its *state* the same piece of text, so
nothing could say what the setting **did** without lengthening the line. Split into
`ui::setting_row` plus `ui::Toggle`, the description has somewhere to live: "Run code on this
machine" is a sentence about trust, not a preference, and the name alone never said so.

`ui::Toggle` is two flex boxes — `justify_end` moves the knob — rather than absolute positioning,
so there is no arithmetic to get wrong at a size nobody re-measures. It also retires one of the
three hand-written exceptions from §67: the checkbox row was only an exception because it was
full width, and a row that is a *row* plus a control that is `flex_none` is the shape that was
wanted all along.

**Still to do, and named so it does not quietly lapse:** Zed's theme picker is a dropdown
*button* that opens a searchable popup, and ours is still an always-open inline list. The
machinery for it exists — `anchored` + `deferred`, from the right-click menu in §64 — and it is
the next thing on this pane. The scrollbar, checked in the same pass, is fine.

## 69. Three ways to say which row is chosen (2026-08-05)

Reported: `ctrl-p`, Enter on **Settings**, nothing happens.

The palette computed its selection in three places and no two of them agreed.

```rust
// what got drawn as chosen
let selected = self.palette_selected.min(commands.len().saturating_sub(1));
// what the arrow keys moved from
let current = self.palette_selected.min(count - 1) as isize;
// what Enter ran
self.palette_commands(cx).get(self.palette_selected)   // <- no clamp at all
```

The render clamped, the navigation clamped, the activation did not. So the moment
`palette_selected` outran a filtered list, the palette drew the last row as chosen and Enter ran
either the *wrong* command or — past the end — nothing whatsoever. All three now go through one
`palette_choice`, so the row that is highlighted is by construction the row that runs.

**And the branch returned in silence**, which is why the report could only be "nothing happens":
a command that never ran and a command that ran and did nothing look identical from outside. It
now says so in the status bar.

Honest about the limit: typing notifies the query entity, whose observer resets the index to
zero, so a *freshly filtered* list self-corrects and I could not reproduce the exact sequence
from the description. What is certain is that one path on the reported route could silently do
nothing, and it cannot any more. If it recurs, the status line now distinguishes "no command
matched" from "the command ran".

Third time this shape has appeared: two call sites deriving the same fact separately and drifting
(§67's `disabled` colour beside its own guard, §60's fix verdict read from output instead of the
re-check). The fix is always the same — one function, both callers.

## 70. The UI polish list, and the item on it that was wrong (2026-08-05)

Six entries had been sitting under "UI debt" since §58. Five are done; one turned out to name
the wrong fix, and one is deferred with a reason rather than quietly dropped.

**The theme and model pickers are dropdowns.** A trigger showing what is chosen, opening a
searchable popup — Zed's shape, and the reason shows the moment there is more than a handful:
a hundred installed palettes is a hundred rows in a window that has four other settings in it.
The list itself did not change; it moved into a popup and gained a trigger, so hovering a row
still previews the palette (§50). Built from the same `anchored` + `deferred` pair as the
right-click menu (§64).

**Fenced code has a monospace face, and nothing was bundled.** A font is a multi-megabyte
binary, a licence to carry, and a rendering result nobody here can look at — and it buys nothing
on the platform that matters, because `Consolas` has shipped with every Windows since Vista.
`gpui::Font` takes `fallbacks`, so this is a stack: `Cascadia Mono`, then `Consolas`, then
`DejaVu Sans Mono` for the development machine. Zero bytes added. `HighlightStyle` carries no
font family, so this reaches fenced blocks only — *inline* code stays marked by colour, which is
what it already was.

**Focus rings.** Which field has the keyboard was invisible: there is a caret, and it is two
pixels wide. The composer and every settings field now light their border. `in_focus` rather
than `focus`, because the thing holding focus is a child entity and the border belongs to the
box around it — and `track_focus` is what ties the two together. That same handle is what makes
`tab_index` work: Tab walks the fields of a page in reading order and lands *in* the field
rather than on a div that swallows typing.

**Toasts.** The status bar holds one line, so "copied 12 lines" was routinely overwritten before
anyone looked at it. Outcomes now stack in the corner and fade on individual timers — a second
message must not cut the first one's time short — capped at three, and clickable to dismiss.
Deliberately only for things a *person did*: streaming progress still goes to the status bar,
because a toast per token is a wall of them.

**Resizable panels.** Both widths were fixed numbers. 240px of conversation list is generous on
a laptop and mean on a 4K monitor, and the person who knows which is the one looking at it. The
drag is tracked on the *root*, not on the four-pixel strip, because the pointer outruns four
pixels immediately. Clamped to 160–640: a pane dragged to nothing is one with no handle left to
drag back.

### The item that was wrong

The list said **"`uniform_list` for the transcript — every message is laid out every frame."**
The cost is real. The remedy was not: `uniform_list` "measures the first element and lays out
all remaining elements based on that measurement" and "only works for elements with uniform
height". Transcript messages are a one-line question and a two-page report. It would have
rendered every message at the height of the first.

The variable-height element is `list` + `ListState`, and it wants `splice` on every height
change — which for a message growing a token at a time is a splice per token, to virtualize a
list that is rarely more than a few dozen items.

So the actual cost got fixed instead: **the markdown was being parsed in `render`**, so every
message in the conversation was re-parsed sixty times a second for text that had not changed
since it arrived. Blocks are now cached beside the body and rebuilt only when it changes — once
for a finished message, once per token for the single one still streaming. Virtualization stays
on the list, honestly described, for when a conversation is long enough to need it.

### Deferred, and why

**SVG icons** instead of `◎ ▤ ▥ ⏎`. It needs an `AssetSource` registered on the app and a set of
hand-authored SVG files committed, to replace glyphs that render correctly today, for no
functional gain — and I cannot look at the result. It is the one item on this list where the
work is all risk and the payoff is all taste, so it waits for someone who can see it.

## 71. Four bugs from one pass of using it (2026-08-05)

§70 shipped five things at once. A single session with the app found four defects, and three of
them were invisible to every test in the suite because each lived in an *interaction* between
two pieces that are individually correct.

**Copy was greyed out over text that was visibly highlighted.** The span registry is rebuilt
each frame, and it was cleared at the top of `render`. But `render` is also where the
right-click menu decides whether to grey its Copy row — and it asks *before* prepaint has
registered anything. So the question "is there text to copy?" was answered against an empty map
every single time. `ctrl-c` kept working, because a key handler runs between frames when the map
is full, which is exactly the kind of half-working that hides a bug.

The registry is double-buffered now: prepaint fills the next frame's map while everything else
reads the last completed one. Nothing is ever read while half-populated.

**Escape stopped closing the preferences window.** Opening it focused `fields.first()` — the
Model page's first field. On Appearance or Setup that element is not rendered at all, and focus
on an unrendered element means key bindings stop arriving. Hence the shape of the report:
Escape did nothing *until you clicked Model*, which put that field back on screen. Focus now
goes to the first field of the page being shown, and pages that have no fields focus the window
itself — which is also why `Modal` gained a focus handle. Changing page moves focus for the same
reason. It is very likely the same fault behind "the focus ring doesn't work": the ring was
drawn correctly on an element nothing was focusing.

**A fenced code block rendered as an empty box, a stray paragraph and a second empty box.** All
three at once, which is the tell. An answer showing what a fenced block looks like wraps it in a
*longer* fence — four backticks around three — and the parser closed on any ` ``` `. So the outer
fence closed on the inner opener (empty block), the example became a paragraph, and the inner
closer opened a second fence. Fences are now measured, and close only on one at least as long,
which is what CommonMark says and what the failure was already telling me.

Worth noting how close I came to fixing the wrong thing: the only recent change to that code path
was the monospace font, and "empty box" reads exactly like a font that failed to resolve. Testing
`parse` before touching anything is what stopped a font revert from being shipped as a fix for a
parser bug. The font *did* get one change, but a defensive one: the primary family is now chosen
per platform from ones that are always installed, because `fallbacks` covers missing glyphs and
is not a promise about a missing family.

**Two identical filter boxes in the theme popup.** `theme_list` brings its own, and the popup
added another. Plainly visible in a screenshot, invisible to a test suite that never assembles
two components together.

The pattern across all four: every one is a *seam*. Render order against prepaint order, focus
against what is rendered, a parser against its own output, one component against another. The
unit tests were right about each piece and had nothing to say about any of the joins — which is
the second time in three days that a real screenshot found in seconds what the suite could not
reach (§66 was the first).

## 72. Two boxes that read identically and were not (2026-08-05)

The theme popup's gallery search rendered a quarter of the width of the filter box above it,
with its placeholder spilling out the right-hand side.

Both were `div().px_2().py_1().rounded_md().bg(…).border_1()` around a composer, and neither
said `w_full`. Both were relying on flex stretch to fill their column, and only one got it. I
did not chase down which nesting difference decided that, because the answer would have been a
fact about taffy rather than about this app: the fix is that a text field states its own width
instead of inheriting an argument.

So the gallery box is now the same `filter_field` as the theme box — one function, both callers
— and it gained the focus ring the hand-written copy never had. That is the fourth time this
week the fix has been "these two places compute the same thing separately" (§60, §67, §69), and
the third where making it one function removed a bug nobody had reported yet.

## 73. Proposed — the provenance graph (2026-08-05)

Requested: a modal showing which subagents a conversation used and how it moved between them.
The example given is the point of it:

> paper search → theories → get data → clean data → analyze data → theories → paper search

"This shows how in reality science works, so each scientist can track his work by conversation."

That sentence is the specification. Not a picture of the machinery — a **record of the enquiry**:
what was consulted, what it led to, and where the work doubled back. The doubling back is the
part worth building for. A run that goes out to the literature, forms a theory, gets data,
analyses it, and *returns to theories with what it found* is not a pipeline that malfunctioned;
it is the loop the whole method is made of, and it is invisible today.

### It is not a DAG, and the request already says so

"DAG-like (I think there can be cicles)" is exactly right, and it settles the data structure. The
acyclic assumption is what would have to be forced on this, and forcing it would delete the one
edge a scientist most wants to see — the return to a step already taken. So: a **directed
multigraph over subagent kinds**, cycles allowed and expected.

- **Node** = a kind of subagent (`paper search`, `theories`), *not* an invocation. `theories`
  appearing twice in the example is one node visited twice, which is what makes the loop visible
  at all. Visit count belongs on the node.
- **Edge** = an observed transition, with a count. `theories → paper search` traversed three
  times is one edge that says three, not three edges.
- **Order** = across the whole conversation, not one turn. The loop in the example spans turns.

### What exists, and the one thing that does not

The live data is already there. `Message.agents` is `Vec<AgentTrace>`, one entry per invocation,
carrying `ns` (unique per invocation, from the pregel checkpoint namespace) and `name` (the
display name the backend sends as `lc_agent_name`, §15b). Entries are appended in first-seen
order, so a turn's sequence is recoverable, and `Message` order gives the conversation's.

**What does not exist is any of it after a reload.** `conversation_messages` returns role and
text only, and says why: *"the activity trace is not replayable — it was assembled from a stream
that is over, and pretending otherwise would show an empty trace next to a real answer."* That
was the right call for the transcript. It is fatal here: a graph that empties when you reopen the
conversation is not a record of your work, and "track his work by conversation" is precisely the
thing it would fail at.

**So this feature is a persistence problem before it is a drawing problem**, and that ordering
should survive contact with the fun part. The graph is appended to as a turn streams and written
to the thread's own directory — `Documents\Mini-Me\<thread>\provenance.json`, beside the outputs
that turn produced (§42's directory, already ours, already the place a researcher is pointed at).
Written by the client because the client is the only thing that sees the stream.

### The honest limit, named before it is discovered

Adjacency in that list is **arrival order, not causation.** Subagents run concurrently — the `ns`
namespace exists precisely so two concurrent runs of the same kind stay in separate groups — and
nothing carries a timestamp or a parent. So `A` appearing before `B` may mean A led to B, or may
mean both were dispatched together and A's first token arrived first.

Three options, in order of preference:

1. **Ask the backend for the edge.** A delegation is a `task` tool call made *by* something; the
   coordinator knows which. If the stream can carry the parent, the graph stops being inferred.
   This is the only version that is true rather than plausible, and it is a small upstream
   question — worth asking before building the inference.
2. **Draw concurrency as concurrency.** Group invocations that overlap into a band rather than a
   chain, so the picture says "these two ran together" instead of inventing an order.
3. **Label the inference.** If the edges stay heuristic, the modal says so in one line. A
   provenance record that quietly guesses is worse than no provenance record, because it will be
   believed.

### Drawing it

GPUI has no graph layout and there is no crate to add. Both pieces needed do exist: `canvas()`
for arbitrary drawing (`elements/canvas.rs`) and `window.paint_path` for the edges. Two stages:

- **First, the chain.** Chips with arrows between them, wrapping — literally the notation the
  request was written in, which is a strong hint that it reads. Revisits repeat the chip; a
  repeated chip *is* the cycle, legible with no layout algorithm at all. This is a day's work on
  data that already exists and answers the question as asked.
- **Then the graph**, if the chain proves it wants one: nodes placed by first appearance, edges
  as curves, thickness by traversal count. Layered layout by hand — a real algorithm, and only
  worth it once the chain has shown which conversations are complex enough to need it.

Modal rather than a panel, and reached from the command palette and the conversation's row: it is
something you open, read and close, which is the same argument §68 used to move Setup. `ui::Modal`
already has the shape.

**Sequenced after `/subagent`**, deliberately. `/subagent` gives a researcher a way to *invoke*
work deliberately rather than hoping the coordinator delegates; this shows what was invoked.
Building the record first would mean building it twice — once for the delegations that happen
now, and again for the ones a researcher asks for by name.

## 74. Intervals, not arrows — how the provenance graph should be built (2026-08-06)

§73 said "nothing carries a timestamp or a parent" and used that to justify inferring edges from
list order. The reply was that timestamps exist. Checking sharpened it in a way that changes the
design:

- The **client parses no per-event time.** True, and the part that matters.
- The **captured fixture has none either** — zero hits for `created_at`, `timestamp`,
  `start_time`, `end_time`. But that capture is explicitly *reduced* ("chunk/metadata narrowed to
  the fields any client reads"), so it is evidence about our decoder, **not** about the wire.
  §73's flat claim was therefore over-stated: the wire is an open question, to be answered by a
  fresh `MINIME_CAPTURE_SSE` with nothing dropped.
- And the decisive point: **the client can stamp arrival itself.** Every event already passes
  through one decoder. First and last token of an invocation give it an interval, for nothing,
  with no backend change and no permission to ask for.

### What an interval buys that an arrow does not

With a start and an end per invocation, the two cases separate on a *fact* rather than a guess:

- **Intervals overlap** → they ran together. Draw them as a band. No edge, because there is no
  "then".
- **A ends before B begins** → "B started after A finished". That is a true sentence, and it is
  the edge.

So the edge stops being invented. §73's worry — that adjacency in a list would manufacture an
order between two subagents dispatched at once — disappears, because overlap is now visible
instead of flattened.

Stated precisely, since this is a record scientists will trust: an arrival interval is *narrower*
than the execution it stands for. The first token lands after the agent started, the last before
it stopped. So **overlap proves concurrency**; a gap only *suggests* sequence — A may still have
been working silently while B streamed. Worth one line in the UI, not worth pretending away.

Causation remains out of reach either way, and that is fine: "B started after A finished" is what
a lab notebook records too.

### So: a timeline, with the graph as its projection

The better thing to build is **both, from one structure** — and the timeline first, because it is
made only of facts:

- **The timeline is the record.** Each turn is a row, headed by the question that started it;
  each invocation is a bar. Parallel work looks parallel. Duration is visible, which answers a
  question nobody has been able to ask yet — *which step is slow* — and needs no graph at all.
- **The graph is the shape.** Nodes are kinds, edges are the ordered pairs the intervals justify,
  thickness is traversal count. This is what shows the loop that prompted the request, and it is
  a *projection* of the timeline rather than a second source of truth.

One modal, two views, one dataset. The timeline earns its keep on the first conversation; the
graph earns its keep on the tenth, when the loop is the thing worth seeing.

### What to record

Per turn: the prompt, and when it was sent. Per invocation: kind, `ns`, first-token time,
last-token time. Wall-clock (`SystemTime`) rather than `Instant`, because it has to survive being
written to disk — §73's `provenance.json` in the thread's own directory, still the load-bearing
part, since none of this replays from the backend.

Two questions still worth putting upstream, both cheap and both turning a good proxy into the
real thing: does the stream already carry per-event times, and does it carry the **parent** of a
delegation? A `task` tool call is made *by* something and the coordinator knows what. If that
came down the wire, the edges would be causal instead of chronological, and this section would be
an implementation detail rather than a compromise.

## 75. The parent edge was already on the wire (2026-08-06)

Two questions were put to a capture. Both are answered from source and from the capture already
in the repo, and the second one overturns §74.

### There are no timestamps, and this time that is certain

`langgraph_api/stream.py:262` yields the metadata chunk as exactly:

```python
yield "metadata", {"run_id": run_id, "attempt": attempt}
```

which is byte-identical to the first frame of `delegated-turn.sse` — so on this point the capture
was **not** reduced, and §74's hedge ("evidence about our decoder, not about the wire") can be
retired. Stream events are `(name, data)` pairs with no envelope. `created_at` exists in
`langgraph_api/schema.py`, but on runs and threads — REST resources — never on a stream event.

So durations can only ever be measured *here*, by stamping arrival. That remains worth doing, and
it is the only reason left to do it.

### The parentage is already arriving, and it is causal

LangGraph attaches this to every task (`langgraph/pregel/_algo.py:654`):

```python
metadata = {
    "langgraph_step": step,
    "langgraph_node": name,
    "langgraph_triggers": triggers,
    "langgraph_path": task_path[:3],
    "langgraph_checkpoint_ns": task_checkpoint_ns,
}
```

and `NS_SEP = "|"`, `NS_END = ":"` (`langgraph/_internal/_constants.py:87`). The namespace is
therefore a **path**, and the capture contains a two-segment one:

```
"langgraph_checkpoint_ns": "tools:d6c187d3-…|model:61d75bb5-…"
```

The parent of any node is its namespace minus the last segment. That is a *causal* edge — who
delegated to whom — not a chronological guess about who spoke first. §73 called getting this "a
small upstream question worth asking"; it turns out there is nothing to ask. It has been arriving
since the beginning, and the client already keeps the whole namespace (`AgentRef.ns`) and uses it
only as a grouping key.

### So the design changes

§74's "intervals, not arrows" was the right answer to the wrong question. Intervals were a proxy
for an ordering the engine states outright, and a proxy should not outlive the thing it stood in
for:

- **Edges come from namespace nesting.** Parent → child, true by construction. No inference, no
  disclaimer, nothing to label as heuristic.
- **Concurrency comes from sharing a parent**, not from overlapping arrival. Siblings are
  siblings whether or not their tokens interleave — which also fixes the case arrival intervals
  would still have got wrong: two subagents dispatched together where one stays silent.
- **Arrival stamps survive, demoted to what they are actually evidence of: duration.** How long a
  step took is genuinely useful and genuinely unavailable anywhere else. It is not ordering.

The loop the request was about — `theories → … → theories` — is then read off the *sequence of
turns*, which is where it actually lives: each turn is a root, its delegations hang beneath it,
and a kind reappearing in a later turn is the return. Cycles across turns, a tree within one.
That is a truer picture than a single flat graph, and it falls out of the data rather than being
imposed on it.

### The one thing still unconfirmed

`langgraph_step` is in the engine's task metadata; whether it reaches the *streamed chunk*
metadata is not visible in the capture, which was reduced in exactly that region ("chunk/metadata
narrowed to the fields any client reads"). Absence there proves nothing — the trap §74 already
fell into once. It would order siblings within a parent, which nesting alone does not.

That is now the only reason to run a live capture, and it is a small one: one turn with
`MINIME_CAPTURE_SSE` set and nothing filtered. Worth doing when a turn is being run anyway,
not worth spending a turn on by itself.

**Answering the question as asked: yes, we have what is necessary** — more than was assumed for
structure, less than was assumed for time, and the structure is the half that mattered.

## 76. `/subagent`, part one: the name is real before anything is sent (2026-08-06)

§55 designed this and left one question open: what happens when a named subagent does not exist.
Answering that turned out to decide the architecture, so it went first.

### The registry, and why it is a file

§55 required the list of nameable specialists to come from the backend, not a copy in the client
— "a hardcoded list here would drift the first time upstream renames one". The obvious route is a
`GET /subagents` route. **It cannot work**: `langgraph.json` mounts `http.app` from a *file path*
(`./backend/routes/__init__.py:app`), and file-path loading bypasses `sys.meta_path` — the exact
trap already documented in the overlay, which is why the approval patch had to move onto the
`deepagents` package. A route added by an import hook would never be mounted.

What does work is better anyway. `backend/agent.py` calls
`create_deep_agent(model=…, subagents=runtime_subagents, …)`, and the overlay already wraps that
factory. So `overlay/minime_local/registry.py` records the list **as the coordinator is actually
built with it** — `_build_runtime_subagents` assembles per request, so this reports what can
really be delegated to rather than what a module-level list says. It writes `subagents.json` into
the workspace root both sides already share (`MINIME_LOCAL_WORKSPACE`, the directory figures
appear in, §42), atomically, and never raises: a picker that cannot be populated is worth less
than the turn it would have broken.

There are **ten**, not the seven the upstream docstring claims:
`academic_researcher`, `dataverse_explorer`, `data_cleaning`, `exploratory_data_analysis`,
`diagnostic_analytics`, `predictive_analytics`, `report_writer`, `hypothesis_generator`,
`pdf_librarian`, `data_voyager`.

### The names in the request are not the names in the backend

`/eda-subagent` is `exploratory_data_analysis`. `/research-paper` is `academic_researcher`.
`/report-write` is `report_writer`. Not one of the three guesses is a real name — which is not a
criticism of the guesses, it is the reason completion has to exist at all.

So matching scores each specialist on its name **and its own description**, taking whichever
reads better, with the description worth half. That is what makes `eda` find
`exploratory_data_analysis`: the acronym is in the description, not the name. A test asserts all
five of those mappings against the real registry, so an upstream rename fails a test instead of
becoming a command that quietly does nothing.

### What it is, precisely: not a bypass

`start_async_task` and `task` are **tools the agent holds**. There is no endpoint that runs one
subagent, and building this as though there were would have been the easy mistake. So a
`/subagent` command *composes a turn* that names the specialist, and the coordinator delegates.
Three consequences, all good: the approval gate still applies because nothing went around it
(§19, §41 — and §55's fourth point asked for exactly this); background dispatch will be the same
mechanism with a different instruction rather than a second code path; and it works against the
pinned backend with no upstream change at all.

### Answering §55's open question

Resolved before the turn is sent, and every refusal keeps what was typed:

- **No registry yet** — the backend has not assembled a coordinator, so there is genuinely
  nothing to check against: *"ask one ordinary question first, then /name works"*. Rejecting a
  name that may well be correct would be worse.
- **Unknown name** — refused, and it names the nearest match: *"no specialist called
  "report-write" — did you mean /report_writer?"* Given that none of the three imagined names is
  real, "did you mean" is the useful half of the message.
- **No prompt** — *"say what report_writer should do"*.

### Tested across the seam, because that is where this week's bugs were

The registry crosses a language boundary: written by Python, read by Rust. §71's four defects all
lived in seams, so this one is tested against a **fixture the Python side really wrote** —
generated by running `minime_local.registry.record` over the reference checkout's own
`backend.subagents.subagents`, committed as `tests/fixtures/subagent-registry.json`. The Rust
parser is asserted against those exact bytes, the same way §15's SSE decoder is asserted against
a real capture rather than against a hand-written idea of one.

Twelve tests: the parse (`/` alone is already a command, so the picker can open before there is
anything to match), the ranking, the five name mappings, version refusal, and every malformed
shape a half-written file can present — because another process writes this one while this one
reads it.

### What is left of it

- **The picker.** Typing `/` should open the fuzzy list, which is the whole point of completion
  when none of the names are guessable. The matching and ranking are done and tested; what
  remains is the popup and the trigger.
- **Background dispatch**, which is the more valuable half — three specialists at once. Left out
  deliberately: it has no trigger until the picker exists, and a `Dispatch` enum with one
  reachable variant is API invented ahead of its caller. Same rule that kept `scrolling()` and a
  `Danger` tone out of §67.

## 77. `/subagent`, part two: the picker, and where "background" belongs (2026-08-06)

The names are unguessable — §76 established that none of the three the request imagined is real —
so completion is not a convenience here, it is the feature. Typing `/` now opens the list.

**Above the composer, as a plain flex child.** Not a floating popup: there is no position to
measure, nothing to clip it, and it behaves like part of the field, which is what it is. Same
placement as the approval card and the same reason (§40) — that is where attention already is.
Each row shows the name *and its description*, because none of these names says what it does and
the request's own guesses are the proof.

**Enter completes while the name is being typed, and sends once it is settled.** The picker is
open exactly while the input starts with `/` and contains no space, so the first space closes it —
and a half-typed name can never be sent by accident, because it is never a real one. Two Enters is
the rhythm: one to settle the specialist, one to send the request. That needed no new key binding
at all; the composer's existing `Submit` is intercepted, and if nothing matches it falls through
to §76's refusal so the key is never silently inert.

The composer is now **observed** as well as subscribed. It was only subscribed, so nothing
re-rendered as the name was typed and the list would have filtered on the next unrelated frame.

### Background dispatch is a command, not a syntax

`/name!`, `//name`, a trailing `&` — all of them are punctuation to memorise for no gain, and
whether work blocks is a property of the *work*, not something a researcher should have to encode.
So it is a palette entry: **"Run the named specialist in the background"**, acting on whatever
`/name …` is in the composer. Discoverable by looking, keyboard-reachable, and no new grammar.

The instruction it sends says *"tell me it has started and carry on — do not wait for it"*, and a
test asserts that phrase. Its whole value is that the conversation stays live; a coordinator that
starts the task and then blocks on it has given up the point, and nothing else in the wording
would catch that.

`Dispatch` therefore returns — §76 removed it deliberately for having one reachable variant, and
it comes back at the moment it has two callers. The same rule that kept `scrolling()` and a
`Danger` tone out of §67, applied in the other direction.

### Where this stands

Working end to end: type `/`, pick a specialist, say what it should do, send. Sixteen tests over
the parsing, the ranking, the five name mappings, both instructions, and the exact moment the
picker opens and closes.

Two things a person still has to check, both invisible from here: whether the list reads well
above the composer, and — the real one — whether the coordinator actually honours *"delegate this
to `report_writer`"*. The instruction is a request to a model, not an API call, and no test in
this repo can tell me it is obeyed. That is the next thing to find out, and it wants one real turn.

## 78. The registry inherited a switch it had nothing to do with (2026-08-06)

Reported: two ordinary questions asked and answered, then `/report_writer` — and the picker said
*"No specialist list yet — ask one ordinary question first."*

The message was right about its own state and wrong about the cause. `registry.record` was folded
into the wrapper in `async_agents.install`, which begins:

```python
if not enabled():
    return
```

`enabled()` reads `MINIME_ASYNC_SUBAGENTS` — the **"Let work run in the background"** setting,
which is off by default and for good reason (§14: a coordinator holding tools that point at a
graph the server does not serve would fail mid-task rather than at startup). So with background
work off, the wrapper was never installed, `record` never ran, and the file was never written. No
number of questions would have helped.

Naming what can be delegated to has nothing to do with whether work may run in the background.
The registry now installs its own wrapper, unconditionally, and last — so it is outermost and
sees the arguments as `backend/agent.py` passed them. It only ever *reads* `subagents`.

This is the fifth time this project has produced a bug by attaching a new thing to an existing
thing that happened to be nearby (§67's `disabled` beside its own guard, §69's three selection
clamps, §72's two filter boxes, §60's verdict read from the wrong source). The tell is the same
each time: two facts that should be independent sharing one condition. **Sharing a wrapper is
sharing a condition** — that is the part I did not see, because the wrapper was where the data
happened to pass through.

Nothing in the Rust suite could have caught it: 159 tests pass, the cross-language fixture test
passes, and both were right. The defect was in *which Python function* the call sat inside, which
no fixture describes.

### Confirmed while fixing it

`start_async_task` is the real tool name — `deepagents/middleware/async_subagents.py:361` sets
`name="start_async_task"` — so §77's background instruction names it correctly. That file's own
prompt also says "Report the task ID to the user and stop", which is the same thing §77's
instruction asks for in different words, so the two are not fighting.

### Still true, and worth keeping

The list is written when the backend **assembles a coordinator**, which happens per request. So
one ordinary question is genuinely required before `/` has anything to show, and the picker says
so. Seeding it at server start by importing `backend.subagents` directly was considered and
rejected: it would add a second source of truth and an import of upstream's module into the graph
load path, risking a circular import during startup — a real hazard traded for a ten-second
detour that the message already explains.

**Restart the app** to pick this up. The overlay is Python, loaded when the sidecar starts, so a
`git pull` alone changes nothing about a running backend.

## 79. The backend the app attaches to is not the backend it shipped with (2026-08-06)

`/report_writer` still said "no specialist list yet" after §78's fix, a pull and a restart of the
app. The fix was correct. It was not running.

```rust
pub async fn ensure_running(&mut self, client: &LangGraphClient) -> Result<String> {
    if client.is_healthy().await {
        return Ok("attached to a running backend".into());
    }
```

**The app attaches to a healthy backend rather than replacing it.** That is right for speed — a
warm server answers instantly, which is why `warm_up` exists at all (§50). It is wrong after an
update, because the Python overlay lives *in that process's memory*. The launch command already
re-copies the overlay into the distro on every start (`sync_overlay_command`, added for exactly
this reason), and it makes no difference to a server that is not restarting.

So restarting the app reloads nothing, and the only way to reload the overlay was to quit and make
sure nothing survived — which nothing in the app could ask for, and no message suggested. The
symptom is a feature that silently does nothing, which is how §78 presented too. **Two different
causes, one indistinguishable symptom**, and I diagnosed the first one correctly and then watched
the same screen say the same thing.

`Drop` already knew how to tear the backend down properly, including `pkill -f 'langgraph dev'`
because killing `wsl.exe` does not reap what it fronted. That was the whole implementation, sitting
in a destructor where nothing could call it. It is now `BackendSupervisor::stop`, and
`Sidecar::restart_backend` stops, waits for the port to come free, and starts again — reachable
from the palette and from a **Restart backend** button on the Setup page, beside Re-check, because
that is where someone goes when something is wrong and "restart it" is the second thing anyone
tries.

The picker's dead end now names the cause and the action rather than just its own state.

### "The app seems ready when the backend is not"

Half of this was already true and invisible. `warm_up` starts the backend at launch (§50), so the
delay is concurrent with reading the window rather than added to the first question — but nothing
said so, and the window looks finished.

The other half was a **message that lied**. The conversation list said *"Conversations you start
will appear here"* whenever it was empty, and it is empty for the seconds a cold `langgraph dev`
takes to boot — so a researcher with four conversations was told they had none, and then watched
them appear. That is the whole of "conversations take too long to load": they did not take longer
than the backend, they were misdescribed while waiting for it. Loading and empty are now different
states, and a failed fetch stays "loading" because the next refresh will answer.

Not fixed, and named so it is not mistaken for fixed: the backend genuinely takes seconds to boot
because `langgraph dev` imports the graph, and nothing the client does changes that. What the
client can stop doing is looking finished while it happens.

### The pattern, for the sixth time

§78 called it: two facts that should be independent sharing one condition. This is the same shape
one level out — **"is a backend reachable" and "is it the backend this app shipped with" are
different questions, and only the first was ever asked.** Every symptom of the second one is
identical to a feature being broken.

## 80. Where the conversations actually live, and why the wait is what it is (2026-08-06)

Asked directly: why is loading conversations slow, and where are they stored? Both answers come
out of `langgraph_runtime_inmem`, which is what `langgraph dev` runs on.

**They are stored on this machine and nowhere else.**
`langgraph_runtime_inmem/checkpoint.py:59` sets
`filename = os.path.join(".langgraph_api", ".langgraph_checkpoint.")`, relative to the server's
working directory — so for this app that is
`~/.local/share/mini-me-desktop/backend/.langgraph_api/.langgraph_checkpoint.N.pckl`, inside the
distro. Python pickles. Nothing is on anybody's server, and nothing leaves the machine.

**Nothing about listing them is slow.** The runtime is *in-memory*: `d.load()` unpickles the whole
store into RAM at startup, and every later read is a dictionary lookup. `GET /threads/search`
answers instantly once the server is up.

**So the wait is the server booting, and it has two parts.** `langgraph dev` imports the graph —
which imports deepagents, LangChain, the MCP tool wiring and ten subagent definitions — and it
unpickles the whole accumulated store. The first is fixed. The second **grows with history**, and
that includes every background worker's thread (§51 found dozens of those in the sidebar for the
same reason). A researcher who has used this for months boots slower than one who installed it
yesterday, and no amount of client work changes either number. `rm -rf .langgraph_api` in the
checkout resets it, at the price of every past conversation.

What the client *can* stop doing is looking finished while it happens, which §79 fixed: the list
now says "loading" rather than claiming there are none.

### The state that should have been visible from the start

§78 and §79 were two different bugs presenting one symptom — a feature that silently did nothing —
and in both cases the invisible fact was the same: **the backend the app is talking to is not
necessarily the backend the app shipped with.** `ensure_running` answers "is one reachable"; it
never answered "is it mine".

It does now. `Started::{Attached, Spawned}` is a typed answer rather than a sentence — matching on
prose to discover this is how the two get confused in the first place — and when the app attaches
to a server it did not start, the Setup page says so and points at the Restart button beside it.

That is the sixth appearance of one shape in this project, and the clearest: a question that was
never asked because a nearby question had already been answered.

## 81. "Blocking call to os.mkdir" (2026-08-06)

The log line that ended it:

```
minime_local: could not record the subagent registry: Blocking call to os.mkdir
```

`create_deep_agent` is called from inside `async def agent(config)`, so `record` ran **on the
event loop**, and the LangGraph dev server activates `blockbuster` — a guard that raises on
synchronous I/O there. `os.makedirs` and `open` are exactly that. My own `except` caught it and
logged it, and the turn carried on: the tolerance was doing its job, and the file was never
written.

The guard is right, and the point is not pedantic: a synchronous write on the loop stalls health
checks and every other run in the process. `langgraph dev --allow-blocking` would have silenced
it, and taking that would have been fixing the smoke alarm. So the write now goes to a worker
thread when there is a loop, and stays inline when there is not — which is what a plain
`python -c` import does, and what the earlier manual checks were unknowingly testing.

**Verified against the real guard**, not against the reasoning: `blockbuster 1.5.26` is in the
backend's own venv, so the fix is exercised with `BlockBuster().activate()` in the loop. It no
longer trips, and the file lands.

### What actually found it

Four attempts at this feature failed, and the fourth was diagnosed in one reading. The difference
was not insight — it was that §80 added **a log line on success**. Until then `registry.install`
logged only on error, so "the code is absent", "the wrapper never ran" and "it ran and failed
silently" produced identical evidence: nothing. With the install line present, the log said the
wrapper was armed and named its target file, which left exactly one thing that could still be
wrong, and the next line said what it was.

That is the same lesson as §60 (a fix that reported "done" over a red row) and §69 (a palette key
that returned in silence), now paid for three times. **A component that only speaks when it fails
cannot be distinguished from one that was never reached.**

The pull-based redesign floated in §80 — the app asking Python for the list during preflight — is
**not** needed. It was proposed because push had failed three times for three different reasons,
which looked like evidence against the design. It was evidence against the *instrumentation*. The
mechanism was sound each time.

## 82. What the store is, what the checkpointer is, and why not Postgres (2026-08-06)

Asked directly, after §80 answered *where* conversations live but not *what holds what*. Two
different things sit in `.langgraph_api/`, and confusing them is how a "let's use Postgres"
decision gets made for the wrong reason.

**The checkpointer is one conversation.** Every message, tool call, interrupt and resume in a
thread, pickled to `.langgraph_api/.langgraph_checkpoint.N.pckl` — sharded, written by the
runtime with no involvement from this app or the overlay
(`langgraph_runtime_inmem/checkpoint.py:59,69`). It is what the sidebar reads through
`GET /threads/search`, and it is the thing that grows with history, which makes it the whole of
§80's boot cost.

**The store is everything that outlives a conversation.** `DiskBackedInMemStore(InMemoryStore)`
— so the answer to "are we using an `InMemoryStore`?" is *yes, a disk-backed subclass*. It swaps
`_data` and `_vectors` for `PersistentDict`s over `store.pckl` and `store.vectors.pckl`
(`store.py:83-84`), flushed by a daemon thread every ten seconds (`_persistence.py`). Three
namespaces live in it, and `backend/agent.py:64` routes the first two there with a
`CompositeBackend` while everything else goes to the sandbox:

- `/memories/` → `(assistant_id, user_id)` — per-researcher scratch memory
- `/skills/` → `("skills", assistant_id)` — shared across users of one assistant
- the project spine → `(user_id, "project")` — deliberately *not* keyed by assistant, so it
  spans every thread and the `/project` route can rebuild the namespace from
  `request.user.identity` without resolving an assistant_id it never sees

`StoreBackend(store=None)` holds no store; it calls `get_store()` at request time and takes what
the server runtime provides. Which is why none of this needed wiring from the desktop app.

The line between them, in one sentence: **checkpointer is within a conversation, store is across
conversations.** `rm -rf .langgraph_api` takes both — every past thread *and* every memory.

### Postgres: no, and the reason matters

The question was whether a real store would be faster. It would make things **slower**, and the
temptation comes from mistaking §80's boot cost for a store problem.

1. It needs `langgraph up --postgres-uri`, which needs Docker. On machines where WSL2 alone took
   §57–§60 to install, a second required install is a second way to fail.
2. Every read today is a dict lookup in RAM. Postgres makes each one a network round-trip — the
   runtime cost goes up, permanently, on every turn.
3. It helps exactly one thing: not having to unpickle at boot. That gain is real and it is the
   only one, against those two costs.

The actual fix for slow boot is **pruning** `.langgraph_api/`, most of which is background
worker threads (§51 found dozens in the sidebar for the same reason). Measure before deleting.

### A docstring that is load-bearing and wrong

`make_backend`'s docstring says that under `langgraph dev` the store "loses content on process
restart", and advises a durable store so memories survive. It has not been true for as long as
the runtime has shipped `DiskBackedInMemStore`: persistence is on unless
`LANGGRAPH_DISABLE_FILE_PERSISTENCE=true`, which nothing here sets.

Ordinarily a stale docstring is worth a shrug. This one is not, because §79 added a **Restart
backend** button and the Setup page tells people to press it. Believing the docstring means
believing that button discards a researcher's memories. Owed upstream.

## 83. The provenance record, built (2026-08-06)

§73 asked for it, §74 and §75 argued out how the edges should be derived, and this section is the
build. Sequenced after `/subagent` deliberately, and that sequencing paid: the delegations a
researcher now asks for *by name* are recorded by the same code as the ones the coordinator makes
on its own, because there was never a second path.

### The record is the feature; the drawing is a view of it

§73's ordering held. `crates/app/src/provenance.rs` is data — `Record` → `Turn` → `Invocation` —
with no pixels in it, written to `provenance.json` in the thread's own directory as each turn
finishes. That is the load-bearing part: `conversation_messages` returns role and text only,
because the activity trace was assembled from a stream that is over (§46), so *nothing* about what
was consulted survives a reload unless the client writes it down. It is the only thing that ever
sees the stream.

Written at `finish_turn` and only there, for the same reason the auto-title is: the thread id does
not exist until the turn has run, so before that point there is no directory to write into.

### Sibling order — the gap §75 named, filled without inventing anything

§75 retired arrival-based ordering in favour of namespace nesting, which is causal and true by
construction. It also flagged what nesting does *not* do: **it cannot order two siblings.** Both
children of the coordinator are children of the coordinator whether one ran after the other or
both ran at once, and `langgraph_step` — which would settle it — is not visible in the capture.

That gap matters more than it sounds, because in practice almost every delegation is top-level:
one segment, `tools:<uuid>`. A graph built from nesting alone would have drawn §73's example as
five disconnected chips.

So arrival comes back, in exactly the role §74 established for it and no wider:

- Siblings are partitioned into **bands** by interval overlap. Everything in one band was running
  while something else in that band was; everything in band *n* had finished before anything in
  band *n + 1* began.
- Between bands there is a `Then` edge. Within a band there is none — two subagents dispatched
  together are joined by `+`, not by an arrow.

Two edge kinds, and the modal says which is which: `delegated to` is certain, `then` is the order
things were observed in. §73's third option asked for that line, and it is there because a
provenance record that quietly guesses is worse than none — it will be believed.

### What it looks like

One modal, two views, one dataset (§74). **Timeline** is a row per turn headed by the question,
with a bar per invocation laid out against *that turn's* clock — per turn, because the gaps
between turns are however long the researcher took to read and type, and a shared axis would
squash every bar to a sliver. Duration is on the right, which answers a question nobody could ask
before: which step is slow. **Path** is §73's chain, in the notation the request was written in —
chips and arrows, wrapping, a revisit simply repeating its chip. A repeated chip *is* the cycle,
legible with no layout algorithm at all, which is why the canvas-and-`paint_path` graph §73
sketched has not been built yet. It is worth building when the chain proves it wants one.

### The test that will catch this breaking

`the_same_real_turn_lands_in_the_provenance_record` replays the captured `delegated-turn.sse`
through the **real** decoder into a real `Record`. Not hand-written events: if `AgentRef.ns` or
`lc_agent_name` changes shape, it fails there rather than as an empty modal noticed weeks later.

That test exists because of §81. Four attempts at the subagent registry failed, three of them
diagnosed wrongly, and every one of those failures had the same signature — a component that only
speaks when it fails, so "absent", "never reached" and "ran and failed" were indistinguishable.
The provenance record has the same hazard in a worse form: a modal that shows nothing looks
identical whether nothing was recorded, nothing was written, or nothing was read back. So the
recording path is pinned to measured wire data, `save_provenance` reports a write failure in the
status line instead of swallowing it, and an empty record is drawn as the *sentence* "no
specialist has been consulted in this conversation yet" rather than as an empty canvas.

## 84. "esc close", which it never did (2026-08-06)

Reported alongside a screenshot of the palette missing its new entry: *"when ctrl + p and I press
esc I cannot exit that menu."* Two findings, and only one of them was a bug.

**The missing command was a stale binary.** `Command::ALL` carries `OpenProvenance`, and the
palette's only render path is built from `Command::ALL`. The screenshot shows thirteen entries;
the source has fourteen. Nothing to fix — but worth writing down that the first check was *"is the
code actually wrong"* rather than a fix aimed at a symptom, because §78–§81 lost three rounds to
exactly that reflex.

**Escape was a real bug, and an old one.** `escape` is bound twice:

```rust
KeyBinding::new("escape", PaletteDismiss, Some("Palette")),
…
KeyBinding::new("escape", Dismiss, None),
```

with a comment claiming *"the palette's own binding above is more specific, so it still wins while
it is open."* It does not, and gpui says so plainly:

```rust
fn binding_enabled(&self, binding: &KeyBinding, contexts: &[KeyContext]) -> Option<usize> {
    if let Some(predicate) = &binding.context_predicate {
        predicate.depth_of(contexts)
    } else {
        Some(contexts.len())          // ← no context = deeper than anything
    }
}
```

Matched bindings sort deepest-first, so the context-*less* `Dismiss` outranks the scoped
`PaletteDismiss`. And an action that is handled ends the matter — `window.rs`, in the bubble
phase: `cx.propagate_event = false; // Actions stop propagation by default`. So `Dismiss` ran,
`dismiss()` had no palette branch, and `PaletteDismiss` was never reached. The palette's footer
has read "esc close" since it was written.

The fix does not touch precedence at all: the palette is now closed by `dismiss()`, in its place
in the same inside-out chain every other overlay uses. Depending on which of two bindings wins was
the fragile part; there is now one handler that receives the key however that resolves.

### Proven, not reasoned

`a_context_less_binding_outranks_a_scoped_one` builds the app's real bindings into a
`gpui::Keymap`, asks it what `escape` resolves to under `[Palette, Composer]`, and asserts it is
`Dismiss`. It was checked by inverting the assertion and watching it fail. A gpui bump that
changes the rule now breaks a test instead of a key.

### The pattern, for the seventh time

A comment asserting the opposite of the behaviour is worse than no comment, because it stops the
next reader from checking. This is the sixth confident claim in this project to dissolve on one
reading of the source (§52 has three, §61 and §71 have the others) — and the second where **the
wrong claim was written by me, in a comment, and then believed on re-reading**.

The tell was available the whole time: the footer promised "esc close" and nobody had tested it.
A UI that documents a key is making a claim, and this project now has a test for that one.

## 85. Two rows that lied, and a picture that was not one (2026-08-06)

The provenance modal opened on a real conversation, and both views were wrong in the same way:
each showed something true and drew it so that it read as something false.

### The timeline compared things it had no scale to compare

Turn 2 ran `academic_researcher` for 8.2s. Turn 3 ran `dataverse_explorer` for 32.4s. Both bars
were full width, one above the other, identical to the pixel. Reported exactly right: *"this
doesn't make sense because I asked these in two different prompts."*

The cause was a decision made deliberately and argued for in §83: normalise each turn against its
**own** span, so the gaps between turns — however long a person took to read and type — could not
squash every bar to a sliver. That reasoning is sound. What it missed is that a turn with one
invocation has a span *equal to* that invocation, so its bar is always 100%. The overwhelmingly
common case renders as a constant.

And a constant is not neutral. Two bars of equal length, stacked, with different numbers beside
them, make a claim — that these are commensurable — and the view had no basis for it. **A chart
whose bars carry no information is worse than no chart, because it will be read anyway.**

Fixed by scaling every row against the longest **turn span** in the conversation (`Record::scale`,
tested). Spans rather than individual durations because a turn lays its own siblings out inside
its row, and a smaller divisor would push a later sibling off the end. The gaps between turns stay
excluded, so §83's original concern is still handled — it was the *denominator* that was wrong,
not the exclusion.

### "The other image its not a graph"

Also correct. §73 proposed a chain of chips first and a drawn graph second, on the reasoning that
a repeated chip *is* the cycle and needs no layout algorithm. That was a fair bet and it lost on
contact: a row of chips is a sentence about the work, and what was asked for was its shape.

So the graph is drawn — `gpui::canvas` with `PathBuilder`, which §73 had already established were
the two pieces available.

**Vertically**, which is the non-obvious part and the reason it fits. The specialists are named
`exploratory_data_analysis` and `academic_researcher`; ten of those across the modal's usable
570px is 57px each, so a horizontal row would clip every label or need text painted into the
canvas. A column gives each name the full width, grows to any number of nodes, and leaves the
right-hand gutter for the edges.

Edges bow further right the further they travel, so a transition skipping three nodes cannot be
confused with one between neighbours. Thickness is traversal count. The arrowhead is not
decoration: without it the picture says two specialists are related but not which way the work
went, and "which way" is the entire question. The return edge — the one this feature exists for —
reads as an arc running back *up* the column.

`canvas` cannot lay out text and a `div` cannot draw a curve, so nodes are real elements and edges
are painted beside them. Both derive from the same two constants, `ROW` and `GUTTER`, because two
independent copies of that geometry would drift the first time either changed.

### What both had in common

Neither was a coding error. Both were a **defensible choice whose failure mode only appears on
real data** — per-turn normalisation is right until a turn has one bar; a chain of chips is
legible until you wanted a shape. Every unit test passed throughout.

That is the argument for putting a build in front of the person who asked for it early, rather
than polishing against imagined data. The plan had already written down that the graph should
wait until "the chain proves it wants one". It took one screenshot.

## 86. Three things one screenshot showed (2026-08-06)

### The theme picker was 400px wide and declared 320

`ui::picker_popup` sets `.w(px(320.))`. The panel rendered at nearly 400, pushing the filter field
and every colour swatch off the right-hand edge — which is why the search box "had a bug in the
width": it was `w_full` of a parent that had silently grown, so most of it was off-screen.

The cause was one line with no width constraint at the bottom of the theme list:

```rust
.child(format!("Or drop a Zed theme .json in {}.", settings::themes_dir().display()))
```

`C:\Users\LENOVO\AppData\Roaming\mini-me-desktop\themes` has no break opportunity, so its
intrinsic width became the panel's minimum. **A declared width is not a promise on its own** — a
flex child's `min-width: auto` lets content override it. `w_full` + `min_w_0` on that line makes
it wrap; `min_w_0` + `overflow_hidden` on the popup itself makes the 320 mean 320 whatever a
future caller puts inside.

Same shape as §72, which was the *other* box in this same picker coming out a quarter-width. Both
times: a size that looked stated in the source and was actually negotiated at layout.

### Side now carries the role

Questions ride right in a bubble, answers run full width on the left as plain prose — the shape
of every chat client, asked for after a side-by-side. The `you` / `mini-me` captions are gone with
it: the alignment already says which is which, and two signals for one fact is one more than the
eye needs. The bubble is capped at 78% so it stays a bubble, and the ragged left edge is what lets
a glance down the transcript separate questions from answers.

The live status line moved to the **bottom of the transcript** while a turn runs. The trace still
sits above the answer it produced — that is the order it happened in and it belongs with its
message — but during a two-minute delegation it scrolls out of view behind the streaming answer,
and the only question a waiting person has is whether anything is still happening.

### The arc was attached to nothing

The graph painted a correct curve, in the wrong place — floating in the right-hand third of the
panel, connected to neither node.

Nothing to do with layout algorithms. `canvas` cannot measure a `div`, so the nodes and the edges
have to agree on where a node *ends* by construction. The column was `flex_grow`, so it took all
the space and the gutter — where arcs anchor — began 170px from the right edge, while the chips
stopped wherever their names happened to. Fixed by making every chip full width inside the column,
so all of them end exactly where the gutter begins.

**Worth naming, since a graph crate was offered:** this was not a case for one. `layout-rs` and
friends compute node *positions*; the positions here were never in doubt for ten nodes in a
column. What was wrong was the seam between a laid-out element and a painted one, which no layout
crate crosses — it would have produced the same detached arc. A real layout algorithm becomes
worth its dependency when the graph is dense enough that hand placement stops reading, and that
is a question the drawing can now answer for itself.

## 87. The composer opens empty (2026-08-06)

Every launch since P6.0 has opened with *"In one short paragraph, what is your role as the Mini-Me
coordinator?"* already typed into the composer. Reported plainly: *"when the app opens this
appear. we should avoid this behaviour of the prefilled text."*

It was a scaffold with a real job. In P6.0 the app could not be trusted to reach the backend at
all, and Enter-with-no-typing was the fastest possible proof that the whole round trip worked. It
outlived that the moment the round trip stopped being in doubt, and what was left was litter: a
stranger's question to delete before your own could be asked, on every single launch.

The constant survives as `CHECK_PROMPT`, which is what it actually is now — what `--stream` asks
when no `--prompt` is given, so a headless check exercises a full turn without depending on the
researcher's data. The composer opens empty and the placeholder says what to do, which is all a
first launch needs.

### On the six searches

Reported in the same breath: *"its weird that it takes too long to search papers."* The trace
showed `academic_researcher · 6 steps · 0 chars` — `search_papers_by_relevance ×2`,
`search_paper_by_title ×2`, `get_paper ×2` — and the turn was stopped before it answered. The next
turn, asking for one known paper, took 2 steps and 942 characters.

That difference is the subagent's own choice of tool calls against the Asta MCP server, and
nothing in this client shapes it. Two things are worth recording rather than fixing here:

- `get_paper` returns a great deal — the last one landed as a 223 KB `.txt` in the thread
  directory. Six round trips of that order is minutes, and the client is waiting on the network,
  not on itself.
- **From the user's side it looked frozen**, because six tool calls produced zero streamed
  characters. That is the part this app can answer, and §83's timeline now does: each of those
  steps is measured, so "which step is slow" stops being a guess. §86 pinned the elapsed time to
  the bottom of the transcript for the same reason.

If the searches are genuinely redundant — two relevance searches and two title searches for one
request — that is a prompt or tool-description question in Mini-Me, and belongs upstream with the
other three. Worth a look with the timeline open, which is now possible for the first time.

## 88. §72 came back, and this time the mechanism got fixed (2026-08-06)

Three faults in one screenshot, and the first is a repeat.

### The filter box collapsed to a sliver, again

`Filter themes` and `Search Zed's theme gallery` both rendered as a ~10px rounded rectangle with
their placeholder painting straight out the right-hand side. §72 saw exactly this on one of those
two boxes, diagnosed it as "relying on flex stretch", and fixed it by adding `w_full`. It came
back on both.

Because `w_full` was never the mechanism. Two facts, and neither alone is enough:

- **`div()` in gpui is `Display::Block`** (`style.rs:734`), not flex. `filter_field` never called
  `.flex()`, so its child had no row to fill.
- **`ComposerElement` asks for `width: relative(1.)`** — 100% of its parent. A percentage needs a
  *definite* parent width to resolve against, and inside `anchored()` — which lays its child out
  as an absolutely positioned, shrink-to-fit flex container (`anchored.rs:111`) — there was none.
  The percentage resolved to nothing, the border drew around nothing, and the text painted anyway.

So the field is now a flex row, and `ComposerElement` sets **`flex_grow = 1.0` as well as** the
percentage. `flex_grow` needs no definite parent — it takes whatever free space the row has — so
the pair holds in either kind of container. That is the difference between fixing the box and
fixing the reason: §72 fixed a box.

The popup's own width, from §86, was fine. It reads as ~400px in the screenshots because Windows
is at 125% scale: 320 × 1.27 ≈ 406.

### Choosing a theme left the list covering the window it had just repainted

The row applied the theme and did nothing else. Choosing is the thing the picker was opened to do,
and a list that stays up over the window it just changed hides the very change being judged. It
closes now.

### Save did not close Settings

`save_settings` wrote everything, set a note, raised a toast — and left the window open. So the
only visible confirmation was a sentence inside a pane the user had already decided to leave,
which is precisely what the toast was added to avoid. Saving is finishing; it closes.

### What links them

All three are the same omission in different clothes: **an action that completes without saying
so, or a size that is stated without being enforced.** The plan has now recorded the second shape
twice (§72, here) and the first four times (§60, §69, §80, here). The lesson that keeps being
re-learnt is that a UI's claim — a declared width, a footer promising "esc close", a Save button —
is a claim the code has to actually make true, and no test catches the ones made in CSS.

## 89. The report that was never a file (2026-08-06)

*"I cannot find the report!"* — and the folder listing proved it: a heatmap, three boxplots, a
scatter, a CSV, a 151 KB search dump, `provenance.json`. Seven files, no report. Meanwhile the
answer in the transcript said *"the report is in the Outputs panel"*, and it was.

Both were true, which is the whole diagnosis. **A report is the one output that never reaches
disk.** Figures are written by a plotting script inside `execute` and found by diffing the
workspace (§42); datasets and downloaded papers are files by nature. A report is neither —
`ReportArtifactPayload` is `{title, markdown}` (`backend/schemas.py:321`), it lives in the run's
state, and the only copy that ever leaves the backend is the one in the `values` snapshot.

Which the client received on every frame of the turn, reduced to a 96-character label for the side
panel, and threw the body away.

So: the markdown is decoded whole and written beside the other outputs the moment it arrives,
under the name the agent itself proposed —
`EDA_Report_Simulated_Potato_Field_Trials.md`. A snapshot arrives many times per turn and carries
every report each time, so the write is skipped when the file already holds exactly that text;
otherwise the modification time — which `images` sorts by and a researcher reads — would keep
resetting to now. Each new report is announced once, because a file appearing silently in a folder
is not something anyone notices, and not noticing was the complaint.

### The PDF was already built, and had never been called

Asked in the same message: *"how we can render it as pdf? Maybe using typst? I think we did it in
the mini me repo."* Right on all three counts. `POST /render-report/{thread_id}`
(`backend/routes/rendering.py:328`) takes `{markdown, title, sources, used_asta}`, converts through
`pypandoc` to Typst, wraps it in a template with a title page and a citation list, resolves image
references against the thread's working directory so the figures land *in* the document, and
compiles with `typst` — host-side, in-process, no LaTeX. The desktop app had never called it.

It does now, from the palette: **Save the latest report as a PDF**. Rendering in Rust instead was
never worth considering — a faithful markdown-to-PDF pipeline is a large dependency and a long
tail of edge cases, and this one is already installed in the backend's venv and already what the
web client uses, so a report rendered from the desktop app is byte-comparable with one rendered
anywhere else.

One detail that would have been a quiet defect: the citation list is built from a **separate**
decode of `sources`, not from the side panel's bucket. Panel items are truncated to 96 characters
to stay scannable, and a bibliography ending in `…` is not a bibliography. Pinned by a test that
asserts the rendered citation outlives the panel's truncation.

### The shape, again

An artifact the backend considers delivered because it is in the state, and a researcher considers
missing because it is not in the folder. Same root as §42, which found figures the agent never
reported, and the same fix: **the client is the only thing that sees both sides, so the client
reconciles them.** What made this one harder to spot is that nothing was broken — the panel listed
it, the agent described it, and every layer was telling the truth about a file that did not exist.

## 90. The conversations were never erased (2026-08-07)

*"when I run git pull; cargo run the conversations doesnt load like this was erased, is this
normal? what if we have an update and the user click on the button update. The conversation will
dissapear?"*

Two questions, and I answered the second one first and got the first one wrong. Worth recording in
that order, because the mistake is the more useful half.

### What actually happened

`dfea94a` — "Keep background workers out of the conversation list" — made the sidebar filter
`POST /threads/search` on `metadata: { minime_conversation: true }` (`protocol.rs:832`). The tag is
written by `create_thread` (`protocol.rs:804`), so it is carried only by threads created *from that
commit onward*. Every earlier conversation stopped matching, the search returned `[]`, and the
sidebar said "Conversations you start will appear here" — which from the outside is
indistinguishable from deletion. `git pull` is what delivered that commit.

Measured on a real checkout: **0 of 30 stored threads carried the tag, and 26 of them had genuine
message history.** The commit message had anticipated the consequence — *"threads created before
this change no longer appear"* — and judged the casualties to be "almost all junk rows". That
judgement was mine and it was wrong.

`adopt_untagged_conversations` repairs it: before the first listing, if the tagged search comes
back empty, every untagged thread **with a title** is adopted. A title is written by
`rename_conversation` from the first question asked and by nothing else — the async-subagent
middleware names none of the threads it creates — so it is exactly the discriminator the tag was
introduced to provide, applied retroactively. It self-cancels the moment one tagged thread exists,
so the cost after the first launch is one request.

### The data-loss paths are real, and neither of them fired

Traced from source while looking for something that turned out not to be there. Both are live and
both belong upstream:

- `checkpoint.py:71-75` registers the `PersistentDict` with the flush loop **before** calling
  `d.load()`. When the load throws, lines 91-97 swallow it and leave an empty dict that is already
  registered; `_persistence.py` calls `sync()` every ten seconds; `PersistentDict.sync()` pickles
  the empty dict and `shutil.move`s it over the real file, under a comment reading
  `# atomic commit`. Ten seconds after a failed load, the history is gone.
- `database.py:167-184` deletes the conversation index outright on any load exception. Unpickling
  it *imports* `langgraph_api.config`, which reads `REDIS_URI`/`DATABASE_URI` at import time — so a
  missing environment variable alone is enough to raise inside `pickle.load` and take the index
  with it.

The `ModuleNotFoundError` branch names the trigger itself: *"Pulled updates that modified class
definitions in a way that's incompatible with the cache."* So the worry behind the question was
sound even though the diagnosis of the symptom was not.

### The mistake, named

I told the researcher their conversations had probably already been destroyed, and offered a chain
of source citations proving it *could* happen. Every link was real. None of it was what happened.
Having proved a mechanism exists, I stopped looking for the simpler explanation — and the simpler
explanation was a filter this project had added five days earlier and documented as safe.

That is a new shape for this document, and worth naming precisely: **a correct proof of a possible
cause is not evidence of the actual cause.** §81's lesson was that silence is ambiguous. This one
is sharper — a confident, well-evidenced answer to the wrong question reads exactly like an answer
to the right one, and it cost a researcher an afternoon of believing their work was gone.

What broke the tie was measuring: counting how many stored threads actually carried the tag. One
count, against the real data, decided between two mechanisms that both "explained" the symptom.

## 91. The adoption fix repeated the bug it was fixing (2026-08-07)

§90 shipped `adopt_untagged_conversations` and claimed it restored the hidden history. An
adversarial pass measured it instead of reading it, and it did not.

`adoptable` accepted a thread if `metadata.title` was non-empty, reasoning that
`rename_conversation` is the only writer of that key and background workers name nothing. Both
halves are true. The conclusion is still wrong, for a reason that was one `git log` away:

> `rename_conversation` shipped in **`4911094`, 2026-08-02** — the *same day* as `dfea94a`, the
> filter it was written to work around.

**No thread old enough to be hidden is new enough to have a title.** Measured against a real store:
1 adoption out of 30, leaving 25 of the 26 threads with genuine history exactly as invisible as
before. The severity argument in §90 — *26 conversations lost* — was answered by a fix that
recovered one.

That is the same shape as the bug, one level down: **discriminating on a marker that postdates the
data it has to identify.** Written twice, five days apart, by the same reasoning each time —
"nothing else writes this key, so it is exact" — with the question *"has it always existed?"*
unasked both times.

The test now is **messages**. Every hidden conversation has human writing in it; a background
worker's thread carries a delegation's machinery and none. It costs one `GET /threads/{id}/state`
per untagged thread on a list bounded at 200, paid on the single launch that repairs the history
and never again. It cannot fail the same way, because it screens on a property of the
conversation rather than on a marker some release happened to add.

### What caught it

Not a re-reading. **Counting.** Loading the real `.langgraph_ops.pckl` and asking how many threads
each rule would actually adopt turned an argument into a number, and the number was 1. Every
confident claim in §90 was individually true; the arithmetic between them was never done.

## 92. The filter field: three fixes, no reproduction (2026-08-07)

Reported a third time — *"This bug persist! the search bar its to thin! Please be careful and check
why that happens!"* — after §72 and §88 had each declared it fixed. So it went to an investigation
with an adversarial pass instead of a fourth guess. **Both the diagnosis and its refutation are
worth keeping, because the refutation won.**

### The proposed mechanism

`ComposerElement` is a taffy **leaf with zero intrinsic width**: `composer.rs:701-711` calls
`window.request_layout(style, [], cx)` with no children and no measure function, which becomes
`taffy.new_leaf` (`gpui/taffy.rs:62-68`), and gpui's measure callback returns `Size::default()` for
a context-less node (`taffy.rs:196-199`). Its only width is `relative(1.)` — a percentage. Taffy
resolves a percentage against the parent's known size; when that is `None`, `maybe_resolve` returns
`None` and `compute_leaf_layout` falls back to `unwrap_or(0.0)`.

So the field's width floor is literally zero, and every width above it is *derived* rather than
*stated*: `anchored()` is shrink-to-fit (`anchored.rs:107-111`), `ui.rs:735` states 320px, and then
four more links re-derive it — a bare `Display::Block` div, two `w_full` percentages, and the
Composer's own auto-width root. A taffy 0.9.0 replay of the exact style set, with one ancestor
content-sized, produced **`FIELD_BOX = 18px`** — 0 content + `px_2` + `border_1`, which is the
"~10px rounded rectangle" exactly.

It also explained the second half, which three sections had conflated with the first: the
placeholder escapes because `composer.rs:784-789` shapes with no wrap width and
`composer.rs:878-881` paints at `bounds.origin` with **no `with_content_mask`**, and `filter_field`
sets no overflow. A zero-width box does not clip its own text. That is a separate defect and it is
real regardless.

### Why the refutation stands

Every citation checked out verbatim. The mechanism still **cannot fire in this element tree**:

- The 320px panel width is not a lucky survivor — `git log -S"px(320.)"` finds it in `43dd19e`,
  older than §72. It has been stated for the entire life of the bug.
- `anchored()`'s shrink-to-fit is defused by its own child: taffy hands absolute children
  `AvailableSpace::Definite(container_width)` (`flexbox.rs:2144-2153`), so the anchored node's
  content size is exactly the 320 its child states.
- The bare `Display::Block` div at `main.rs:1899` is stretched **unconditionally**, not
  incidentally: `align_items: None` and taffy's `unwrap_or(AlignItems::Stretch)` (`flexbox.rs:437`).

So the chain resolves, and the collapse the simulation produced needs a content-sized ancestor this
tree does not contain.

### What that leaves

**§88's stated mechanism is wrong regardless of the outcome** — the percentage does not fail to
resolve inside `anchored()`, and that section says it does. Retracted here.

And the correct next step is **not a fourth style change**. It is one measurement: log
`bounds.size.width` from `ComposerElement::prepaint` (which already receives it) or put
`.debug_below()` on the field, open the picker on the Windows machine at 125% scaling, and read the
number. If it is ~302 the bug is elsewhere entirely — a paint or DPI issue, not layout. If it is 18,
real gpui disagrees with the taffy replay and the next step is dumping the actual taffy styles.

Three fixes have now been shipped against this without one reading of what the box actually
measures. **The measurement costs one run and discriminates between every remaining hypothesis;
each guess has cost a release and a report from the person using it.**

## 93. Planned — the SQLite checkpointer

Proposed by the researcher: *"Maybe its worth the effort to construct our custom store and custom
checkpointer … so we can use the best of rust and accelerate the conversation loading and also
avoid conversations lost."* Right on the problem, and the docs settle a constraint this document
had assumed wrongly.

**`langgraph.json` takes `checkpointer` and `store` keys**, each a path to an async context manager
yielding a `BaseCheckpointSaver` / `BaseStore`, and it works under `langgraph dev`. This is
configuration, not a reimplementation of the server — which is what the §80 Postgres analysis had
implicitly assumed when it concluded the only alternative was `langgraph up` and Docker. That
conclusion stands for *Postgres*; it was too broad about custom persistence generally.

**Adopt a SQLite checkpointer.** It closes both open hazards and the boot cost with one change:

- No pickle, so the §90 failure mode — a load that throws, then a flush that writes an empty dict
  over good data ten seconds later — has nothing to act on.
- Lazy reads, so §80's boot cost stops growing with history. That is the actual answer to "why is
  it slow", and no client-side work could ever have provided it.
- `langgraph-checkpoint-sqlite` already ships `AsyncSqliteSaver`, so this is roughly ten lines of
  glue rather than an implementation of five async methods.

**Not Rust, despite the framing of the request.** The bottleneck is unpickling megabytes at boot
and writing them back every ten seconds — I/O and serialisation, not computation, so there is no
work for a faster language to do. Reaching Rust from Python means PyO3 and a compiled wheel per
platform, which is a new way for the install to fail on machines that already spent §57–§60
fighting WSL2. Rust earns its keep in this project where it already is: the client.

**Store: wait.** Custom stores are documented **alpha** ("may experience breaking changes in minor
version updates"), and replacing the built-in one means owning semantic search and TTL. The store
holds `/memories/` and `/skills/`, it is small, and it is not what makes boot slow. Do the
checkpointer, measure, then decide with numbers.

**The one real obstacle** is that `langgraph.json` lives in the checkout, and §18's whole overlay
design exists because a checkout the app does not own must not be edited. This lands cleanly on the
app-provisioned backend and needs a deliberate decision for anyone pointed at their own clone.

## 94. Owed upstream, consolidated

Four now, all found here, none of them this app's to fix.

- ⬜ `guardrails.py` claims sandbox isolation that host execution does not provide (§18).
- ⬜ The theorizer reports a *guess* instead of the command's real output (§35) — seven rounds, the
  most expensive defect of this project.
- ⬜ `deepagents`' `start_async_task` passes no config, so no self-hosted deployment can give a
  background run its model, key or recursion limit (§38/§39).
- ⬜ `agent.py`'s `make_backend` docstring says the `langgraph dev` store "loses content on process
  restart" (§82). It does not — the dev runtime's store is disk-backed. Load-bearing, because this
  app tells researchers to restart the backend.

And two new ones against `langgraph_runtime_inmem`, both data-loss paths, neither yet observed
firing but both live for anyone running `langgraph dev`:

- ⬜ **A failed checkpoint load silently overwrites good data.** `checkpoint.py:71-75` registers the
  `PersistentDict` with the flush loop *before* `d.load()`. When the load throws, lines 91-97
  swallow it and leave an empty dict that is already registered; `_persistence.py` calls `sync()`
  every ten seconds; `PersistentDict.sync()` pickles the empty dict and `shutil.move`s it over the
  real file under a comment reading `# atomic commit`. Ten seconds after a failed load, the history
  is gone. The `ModuleNotFoundError` branch names the trigger in its own message: *"Pulled updates
  that modified class definitions in a way that's incompatible with the cache."*
  There is also a latent bug in the recovery attempt itself: `os.remove(self.filename)` at lines 88
  and 94 is given the *prefix* `.langgraph_api/.langgraph_checkpoint.` with no `N.pckl` suffix, so
  it always raises and is always swallowed — dead code that hides how bad the outcome is.
- ⬜ **The conversation index is deleted outright on any load exception.**
  `database.py:167-184` has a bare `except Exception` whose remedy is `os.remove(OPS_FILENAME)`,
  with no existence check and no backup.

Both would be fixed by the same principle: **a persistence layer that cannot read its file must
refuse to write it**, not carry on with an empty copy and flush.

## 95. The SQLite checkpointer, built (2026-08-07)

§93 planned it; this is the build. Three pieces, and the shape of them is the interesting part:
**none of it patches Python and none of it touches the checkout.**

### It is configuration, not a patch

Everything else in `overlay/minime_local` works by import hook, because `langgraph.json` loads
`http.app` by file path and so bypasses `sys.meta_path` (§18). The checkpointer needs none of that.
`langgraph.json` takes a `checkpointer` key naming an async context manager, and the app has
generated its own copy of that config since §30 — `make_config.py` reads upstream's, adds the
background graph, and writes `.mini-me-desktop.langgraph.json` beside it. One more key in the same
generator, and the checkout is as untouched as it ever was.

That retires the obstacle §93 named as the only real one. It also means the change is legible: a
researcher can open the generated file and see where their conversations go.

### Optional by construction

`make_config.py` adds the key **only if `langgraph.checkpoint.sqlite` imports**. Without the
package the config is byte-identical to what it was, and the backend keeps the pickle checkpointer
and behaves exactly as before. Naming a checkpointer the server cannot load would convert a missing
optional dependency into a server that does not boot — a strictly worse failure than the one being
fixed.

The Setup pane carries it as a **`Warn`, not a `Fail`**, with a one-click
`uv pip install langgraph-checkpoint-sqlite`: nothing is broken without it, and a red row for
something optional is how a diagnostics pane stops being read.

### What it actually buys

- **Boot stops growing with history.** `PersistentDict` loads every conversation in the
  installation before the server answers anything (§80). SQLite reads rows when asked.
- **A failed load stops being fatal.** §90/§94's chain — registered with the flush loop *before*
  `load()`, exception swallowed, empty dict flushed over the real file ten seconds later under a
  comment reading `# atomic commit` — has nothing to act on. Writes are transactional and
  per-checkpoint, so a version change that breaks one row cannot take the other thirty
  conversations with it.

Deliberately **not** Rust, despite the request being framed that way. The cost is unpickling
megabytes and writing them back — serialisation and I/O, not computation — so there is no work for
a faster language to do, and PyO3 plus a per-platform wheel is a new way for the install to fail on
machines that spent §57–§60 fighting WSL2 alone.

The database sits at `.langgraph_api/checkpoints.sqlite`, **inside the distro**. That placement is
now load-bearing rather than incidental: SQLite's file locking over WSL's 9p mount is not reliable,
so a Windows-visible path is the one location that could corrupt it. Asked directly whether SQLite
was an argument for running the backend natively on Windows, the answer is no — it is an argument
for keeping the database exactly where it already is. The case for a native backend is the WSL
install itself, which is a separate question with a separate experiment.

### Migration, stated plainly

There is none. Conversations already in the pickle stay there; SQLite starts empty and takes
everything from the moment it is switched on. Writing a converter would mean unpickling the old
store — the operation whose unreliability is the reason for the change. The Setup row says so.

### What is pinned

`the_generated_config_extends_upstream_and_gates_the_checkpointer` runs the real `make_config.py`
and asserts both branches: upstream's `http`, `env` and `graphs` survive, the background graph is
added, and the `checkpointer` key appears **only** when the package is available. It is a contract
between two languages — Rust chooses the filename and passes `--config`, Python decides the
contents — and §76 established this pattern after three rounds of each side being individually
correct about a file neither had produced together.

The module logs on **success**, naming the database path. §81 paid for that lesson three times: a
checkpointer that silently failed to take effect would look exactly like one that worked, until
someone noticed their conversations were still slow.

## 96. On by default, because the default is the whole decision (2026-08-07)

§95 shipped the SQLite checkpointer as an opt-in: a `Warn` row in Setup with a one-click install.
The response was one sentence and it was right —

> *"so we did sqlite as default right? remember to think in users rather then in me the
> developer"*

It was not the default, and that made it nearly worthless. **~98% of the people this is for cannot
code.** To take an opt-in they would have to open Setup, read a yellow row about a pickle
checkpointer, understand that a boot which grows with history and a flush that can overwrite good
data are things worth avoiding, and click. Every one of those steps is a filter, and what gets
filtered out is exactly the researcher least able to recover from a lost conversation.

An optional safety feature is a feature for the people who already knew about the hazard.

### What changed

- **`setup-wsl.sh` installs it during provisioning**, right after `uv sync --extra dev`. Every
  new install has durable storage before it ever starts a conversation.
- **The launch command re-checks, on a checkout the app owns.** Everyone already using the app
  provisioned before this existed; a fix that only reaches new installs leaves the current users
  on the store with the failure modes. Guarded by an import check, so after the first launch it is
  one fast subprocess and the install never runs again. `|| true` throughout — a backend that
  starts on the old store is strictly better than one that does not start.
- **Only where the app owns the environment.** `resolve_project_dir` already distinguishes the
  checkout this app provisioned from one a developer pointed it at, and the rule that keeps it
  welcome on someone's own clone is that it runs nothing destructive or surprising there.
  Installing a package is precisely that kind of change, so it is gated on `owned` and pinned by a
  test — the cost of getting that wrong is silent and lands in somebody else's virtualenv.
- **Setup's row stops being a chore and becomes a report.** It still offers the install, because
  the unowned case is real, but nobody on an ordinary install has to act on it.

One test changed meaning rather than breaking: `the_sandbox_path_is_left_exactly_as_it_was` pinned
that sandbox execution adds no launch preamble. The overlay copy and the generated config are
host-execution machinery and still must not appear there — but **storage is not an execution
concern**. Where conversations are kept is the same question whichever side runs the agent's code,
so the install belongs on that path too, and the test now says what it actually meant.

### The shape

This document has recorded plenty of bugs where the code did the wrong thing. This one did exactly
what it was written to do, correctly, with a test — and was still the wrong answer, because the
question was never "can the researcher turn this on?" but "what happens to the researcher who
never opens Setup?"

**A default is not a convenience; it is the decision, taken on behalf of everyone who will never
revisit it.** The right test for any option in this app is not whether it can be found — it is what
happens to someone who never looks.

## 97. Measuring the box, finally (2026-08-07)

Three fixes have shipped against a filter field that renders about ten pixels wide with its
placeholder painting out the side — §72, §88, and a third diagnosis that §92's refutation
dismantled. **Not one of them began by reading what the box actually measures.**

`prepaint` has received `bounds` the entire time. The measurement was one line away through all
three attempts, and each attempt instead reasoned from CSS intuition about a layout engine whose
behaviour was checkable.

So: `ComposerElement::prepaint` now reports its own width. Under 40px it **warns**, naming the
field by its placeholder. Otherwise, with `MINIME_LAYOUT_DEBUG` set, it reports at info. Both
branches speak, because §81 paid three times for the lesson that a component which only reports
failure cannot be told from one that was never reached — "no warning" has to be confirmable as
*measured and fine*, not assumed.

The number discriminates between every hypothesis left:

- **≈302** (÷ the display scale) — the field is laid out correctly and the bug is elsewhere:
  paint, DPI, or something about how the window is composited. Layout is exonerated and §92's
  simulation was right that the collapse cannot happen in this tree.
- **≈18** — real gpui disagrees with a taffy 0.9.0 replay of its own styles, which is a finding in
  itself, and the next step is dumping the styles taffy actually receives rather than the ones the
  source appears to set.

### The companion defect, which is not the width

The text is shaped with **no wrap width** (`composer.rs:784-789`) and painted at `bounds.origin`
with no clip, and `filter_field` sets no overflow. So a field that measures wrong does not
truncate — it draws straight across whatever is beside it. That is why a ten-pixel box appeared to
*contain* a full-length placeholder, and it is why three sections treated one symptom as one bug.

`window.with_content_mask` now bounds the painting. It fixes no width. What it does is convert any
future layout mistake from "text floating over unrelated UI" into "text visibly cut off" — a
symptom that points at its own cause.

### The rule this is really about

**A measurement that is one line away is not an optimisation to reach for after the guesses run
out; it is the first thing to write.** Three releases and three reports from the person using the
app were spent avoiding it. The same shape as §91, where counting how many threads a rule would
actually adopt turned an argument into the number 1 — and as §81, where one log line on success
ended four rounds of misdiagnosis.

Every one of those was cheap, available from the start, and skipped in favour of reasoning about
code that could simply have been asked.

## 98. The generator that could not import itself (2026-08-07)

*"I noticed that when I moved the data to sqlite I see this message and I cannot start any
conversation"* — `backend exited during startup with exit code: 1`.

Mine, and squarely. §95 added one line to the top of `make_config.py`:

```python
from minime_local import checkpointer as sqlite_checkpointer
```

The launch command runs that file **as a script**:
`.venv/bin/python <overlay>/minime_local/make_config.py .`. Python then puts the *script's own
directory* on `sys.path` — `minime_local/`, not the overlay root above it — so the package
`minime_local` is not importable from inside itself. `ModuleNotFoundError`, exit 1, and because the
generator is joined to the server with `&&` (deliberately, §30: a config that failed to generate
must stop the launch rather than start a coordinator holding tools pointing at a graph nobody
serves), the backend never started at all.

Fixed by inlining the three-line availability check. `checkpointer.py` keeps its own copy for the
server, which loads it by file path and needs no package either. Duplication measured against a
backend that cannot start is not a close call.

### The test passed while production was broken

This is the part worth keeping. §95 shipped with a test —
`the_generated_config_extends_upstream_and_gates_the_checkpointer` — that ran the real
`make_config.py` and asserted the real output. It passed. It kept passing while the app would not
boot.

Because it invoked the generator **differently from production**: `python3 -c` with
`sys.path.insert(0, overlay)`, which is precisely the arrangement that makes the broken import
work. The test constructed the one environment in which the bug is invisible, and did so in the
course of being careful — the `sys.path` line was written to make the import succeed, which is
exactly what production could not do.

**A test that exercises a different invocation than production is not testing production.** The
replacement, `the_generated_config_survives_being_run_as_a_script`, shells out to the file by path
with `PYTHONPATH` removed — the same call `generate_config_command` builds. It fails against the
shipped code and passes against the fix.

That is a different failure from §81's and §91's, and worth distinguishing: those were about not
looking. This one looked, wrote a test, and *arranged the conditions under which the answer would
be reassuring*. The closest relative is §92 — a taffy simulation that reproduced a bug the real
tree cannot produce. Both times the model of the system was built to be runnable rather than to be
faithful, and the difference only showed on the real machine.

## 99. 0.0px, 38.4px, 204px, 533px (2026-08-07)

The measurement §97 added, from one real window:

```
INFO  text field width  width=204.0    field=Search conversations
INFO  text field width  width=533.6    field=Ask Mini-Me…
WARN  too narrow        width=0.0      field=Filter themes
WARN  too narrow        width=38.4     field=Search Zed's theme gallery
```

Four numbers, and they settle in one reading what three fixes and two investigations could not.

- **The field is not the problem.** The same `filter_field` helper, the same `ComposerElement`,
  measures 204px in the sidebar and 533px in the composer. Nothing about the widget is broken.
- **The popup is.** Both fields inside the theme picker collapse, and only those.
- **One of them is exactly `0.0`** — not small, not rounded: zero. That is a percentage resolving
  against an indefinite parent and falling back to `unwrap_or(0.0)`, which is precisely the
  mechanism §92's investigation described.

So the investigation was right about the mechanism, and **the refutation was wrong that it cannot
fire here.** It argued the popup's declared 320px is definite and propagates by flexbox stretch,
citing taffy's `unwrap_or(AlignItems::Stretch)`. The reasoning was sound, every citation checked
out, and the window disagrees. Something between the popup and the fields is not carrying the
number down — and 0.0 is proof, not inference.

The chain had exactly one link that states nothing: the popup's content was wrapped in a bare
`div().child(panel).on_mouse_down_out(…)`, which in gpui is `Display::Block` with `width: auto`.
Every `w_full` beneath it was a percentage of that. It is now a flex column with `w_full` — a
stated width, in the one place between the 320 and the fields that had none.

### And a floor, because derived widths keep evaluating to nothing

`ComposerElement` had two widths and both were *derived*: `relative(1.)` is a fraction of the
parent, `flex_grow` is a share of the parent's spare room. Every fix in this saga added another
derived width to a chain whose problem was that it was entirely derived — §72 added a percentage,
§88 added a flex-grow, and the measured result of both was 0.0.

`min_size.width = px(120.)` asks nothing of any ancestor. It does not fix the popup; it means the
worst case is a small control rather than an invisible one, wherever this field is used next.

### What actually ended it

Not insight. `prepaint` has received `bounds` since the element was written, and printing it took
one line. Three releases, three reports from the person using the app, one detailed investigation
and one detailed refutation were spent on a question the program could answer about itself.

§81 was one log line on success. §91 was counting how many threads a rule would adopt. This is one
`f32` from a function that already had it. **The instrument is nearly always cheaper than the
argument, and it is nearly always skipped.**

Still to confirm on a real window: that the two fields now measure like their siblings. The
logging stays in — it costs nothing, and it is the only reason this section exists.

## 100. The filter field, confirmed — and the thumb on the swatches (2026-08-07)

§99's fix is confirmed on a real window: *"the search bar its fixed."* Both fields in the theme
picker now lay out like their siblings, and the `WARN` is gone. Four numbers ended a bug that had
survived §72, §88, an investigation, and a refutation of that investigation.

The logging stays. It costs nothing when there is nothing to say, and it is the only reason there
is a §99 rather than a fourth guess.

### The scrollbar was sitting on the rows

Visible in the same screenshot. `scrollbar` paints at `right: 2px`, `6px` wide, so it owns the last
eight pixels of whatever it is drawn over — and the theme rows ran the full width beneath it, which
put the thumb on their right border and their last colour swatch.

Two numbers that had to agree and never had to be written down together. `SCROLL_GUTTER` states it
once and both sides use it: the bar's geometry on one side, `pr(SCROLL_GUTTER)` on the lists it
covers on the other.

Only the two picker lists needed it. The transcript and the artifacts panel already carry `p_4`,
which is wider than the gutter — so this was never a general defect, just the two places where
content was allowed to reach the edge.

### The smallest version of a pattern this document keeps recording

A constant in one file and a layout decision in another, agreeing by coincidence until one of them
moved. §72's box "stated" a width that was a percentage; §88's `flex_grow` needed a row that was a
block; §98's import needed a `sys.path` the launch never sets. Each time, two things had to hold
together and only one of them said so.

The fix is always the same shape: **write the number once, where both sides can read it.**

## 101. The six reports, written (2026-08-07)

Written up as filable documents in `docs/upstream/`, one per defect, each carrying evidence with
`file:line`, what it costs, and a suggested fix. Not filed: filing is an outward-facing act on
somebody else's repository, and who does it and when is not this repo's call.

Two audiences, and they are not the same kind of ask.

**Mini-Me** gets four, and three of them are about *what a component says*, not what it computes:

- The **theorizer** reports an inferred cause where the command's real output belongs. The CLI it
  wraps fails with exit 0 and empty output, so the two together defeat both "read the error" and
  "log the failures" — six real defects were found and fixed while chasing a seventh that was
  neither. The fix is to print the exit code, stdout, stderr and the *source* of the token; naming
  the source alone would have ended it in one round.
- **`guardrails.py`** tells the researcher a command is sandboxed at the moment they are deciding
  whether to allow it. Under local execution it is not. That is the one sentence standing between
  a person and a command on their own filesystem.
- **`agent.py`'s docstring** says the dev store loses memories on restart; it does not, and this
  app has a Restart button it tells people to press.
- **`deepagents`' `start_async_task`** creates the run with no `config`, so background work
  inherits no model, no key and no recursion limit — fine on LangGraph Platform, broken on every
  self-hosted deployment, and silent because the run is created successfully and fails later
  inside a graph nobody is watching.

**langgraph** gets two, both silent data loss, both reachable by anyone running `langgraph dev`.
They share a single sentence as their fix: **a persistence layer that cannot read its file must
refuse to write it.** One registers an empty dict with a flush loop that overwrites the real file
ten seconds later; the other deletes the thread index on any exception, including one caused by an
unset environment variable.

### What writing them up changed

Two things sharpened in the writing that had been fuzzy in the plan.

The `os.remove` in the checkpoint recovery path is **dead code** — it targets the filename prefix
without the `N.pckl` suffix, so it always raises and is always swallowed. That looks like a
harmless bug and is the opposite: because the delete fails, the file survives to be overwritten
with valid empty data, instead of being left corrupt where a human might notice.

And three of the four Mini-Me reports are the same defect in different clothes: **a component
stating something it does not know.** A guessed cause, a safety claim that is conditionally false,
a docstring describing a behaviour the runtime stopped having. None is a crash. All three cost
somebody hours, because the wrong thing to say is more expensive than saying nothing.

## 102. The work was never lost, only unwatched (2026-08-07)

I had just flagged a limit on the theorizer: the poll that observes a terminal state is also the
thing that persists the result, so closing the window during a 5–15 minute run means nothing local
ever writes it. The reply reframed it:

> *"but asta cli let you call theorizer so you can get metadata from their remote process"*

Which is the whole answer. **The run lives on Asta's hosted service, keyed by a task id.** Closing
the window never stopped the work — it stopped our watching of it. Nothing was lost; something
merely stopped being observed, and those are only the same thing if the id is gone too.

It is not gone. It is in the thread's artifacts, and — this is the part that makes the fix small —
**in a response the client was already making.** `GET /threads/{id}/state` returns `values`
carrying the messages *and* the artifacts, and `conversation_messages` read `values.messages` and
threw the rest away.

So reopening a conversation now decodes that same payload: outputs, spine, and every job's task id
and status. `track_job` re-arms the poll for anything still running, and already declines to poll a
finished one (a guard that existed for a different reason and turned out to be exactly right here).
A theorizer left running yesterday is picked back up on open, and the poll that persists its
theories happens after all.

### The shape, which is becoming familiar

The third time in three days that a response already on the wire was decoded down to the one field
someone had needed at the time:

- §89 — a report's `{title, markdown}` reduced to a title for a side panel, so the only copy of the
  body was discarded and the researcher's folder held no report.
- §99 — a width that `prepaint` had received all along, never read.
- Here — artifacts fetched on every conversation open, parsed for messages only.

None was a bug in the sense of code doing the wrong thing. Each was a decoder written to answer one
question, then reused for a second question it silently could not answer. **The cost is never
visible at the call site**, which is why all three survived so long: nothing was thrown, nothing
was logged, and the missing capability looked like an absent feature rather than a discarded one.

Worth asking of the next decoder: *what else was in that response, and who will need it?*

## 103. About, and crediting Asta (2026-08-07)

Four gaps named against the web app; this is the first two, which are one modal.

**An About window.** Ten specialists delegate to each other, and a researcher meets them one at a
time, in a trace, mid-answer. A list is the cheapest orientation there is — plus where the data
comes from, since Asta, CIP Dataverse, AGROVOC and Crop Ontology are other people's catalogues and
which one an answer leaned on changes how it should be read.

**The team list is read from the live registry** (§76), never written into the modal. That list
exists precisely so a copy in the client cannot drift the first time upstream renames a specialist,
and an About box naming agents the backend no longer has would be that defect wearing a friendlier
face. When the registry is empty it says why — the backend has not assembled a coordinator yet
(§78) — rather than showing a blank space that reads as "there are none".

### The attribution is an obligation, not a nicety

The Allen Institute asks that work using Asta cite AstaBench. A tool that makes their search easy
to use while making the citation hard to find is taking something without saying so. So the
reference is in the modal, in full, and **selectable** — a citation you cannot copy is a citation
someone will retype wrongly.

Beside it, the disclosure this organisation requires of its own people: that generative AI produced
the analysis, and that a subject-matter expert should check it. Both belong in the same place,
because they answer the same question — *what do I owe when I publish this?*

### Where code runs — and not repeating the bug we just reported

The web app's About says every conversation runs in an isolated LangSmith sandbox. **On this app
that is usually false.** Host execution is the default (§11): a local-first workbench shipping the
researcher's own files to a rented VM to be read was the wrong shape.

So the modal reads `sidecar.execution()` and says which one this install is actually in, naming the
folder in the local case. Saying the reassuring thing regardless is exactly the defect this repo
reported upstream against `guardrails.py` a few hours ago
(`docs/upstream/mini-me/guardrails-claims-isolation.md`), and repeating it in the document that
explains the product would be worse than leaving it there.

That is the third component in two days caught **stating something it does not know** — the
theorizer's guessed cause, the guardrail's conditional promise, and now nearly this. The tell is
the same each time: a sentence written once, about a system that later grew a second mode.

### Still to come from the same list

- **Per-subagent model selection.** The backend already accepts it —
  `configurable.model_config.subagents` is a `{name: "provider::model"}` map
  (`backend/models.py:104-122`), and the client sends only `default` today. Client-side work only.
- **Projects in the conversation list.** Conversations are flat; there is no grouping, so a
  researcher with three lines of work has one undifferentiated column.

## 104. A model per specialist (2026-08-07)

The second of the four gaps against the web app, and the one with the most behind it.

**The backend has accepted this the whole time.** `configurable.model_config.subagents` is a
`{name: "provider::model"}` map, read at `backend/models.py:114` and folded into the provider set
the request needs keys for at `:117-122`. The client sent `default` and nothing else. So this is
client-side work only — no overlay, no upstream change, no new route.

### Why it is worth having

The specialists do genuinely different work, and one model for all ten is either an expensive way
to grep or a cheap way to write a paper. Literature search wants a long context window and cheap
tokens across many calls. A report wants the best prose available, once. Data cleaning wants
neither and runs dozens of times a session.

### The part that would have failed silently

The backend derives the providers it needs keys for from the coordinator's spec **and every
override**. Point one specialist at a second provider and that provider's key becomes part of the
request — so sending the overrides without the key produces a turn that dies *inside a subagent*,
several minutes in, reading exactly like the specialist being broken.

So `model_choice` collects a key for every other provider an override reaches, and the picker
**names a provider with no key stored, on the row, before it is chosen**. A researcher finds out
at the moment of choosing rather than at the end of a long turn. It is muted rather than red: a
missing key is a thing to do next, not a thing done wrong.

Two details settled while writing it:

- **"Use default" is not the same as choosing the coordinator's current model.** One follows
  whatever the coordinator becomes; the other is a choice that happens to match today and will not
  move with it. The picker offers both, and `model_choice` drops an override equal to the default
  rather than sending a shape with no effect.
- **The list is the live registry** (§76), so it cannot offer a specialist the backend does not
  have — the same argument that put the registry there, and the same one behind §103's About list.

### And a small consolidation

The theme list, the model list and the new per-specialist list had drifted into three shapes for
the same row: a label, a tick, a hover. `picker_row` is now one function, which is also where the
"no key stored" note lives, so a future picker cannot forget to say it.

### Where this leaves the four

- ✅ Per-specialist models.
- ✅ An About window, with the Asta attribution (§103).
- ⬜ **Projects in the conversation list.** The remaining one, and the only one that is a data
  question rather than a screen: is a project a folder on disk, or a name on the thread? Claude
  Code groups by working directory, which is real but does not map — a researcher's three lines of
  work all sit under `Documents\Mini-Me`. A name in thread metadata, grouped in the sidebar, is
  the shape that fits what is already there.

## 105. Projects are folders (2026-08-07)

The last of the four gaps, and the only one that was a data question rather than a screen. Asked
whether a project should be a label on the thread or a real directory, the answer was folders —
*"I think working in folders makes more sense in science"* — and that is right for a reason this
document had already written down once.

§42 moved outputs out of the distro into `Documents\Mini-Me` on a single argument: **files a
researcher cannot find are files that do not exist.** A project is the unit a scientist actually
works in, so the same argument applies one level up. A grouping that lives only inside the app is
not a grouping they can zip, back up, or drop on a shared drive.

### Both, doing different jobs

Folders and metadata were framed as alternatives. They are not:

- **The folder** is where the files live: `Documents\Mini-Me\<project>\<conversation>\`.
- **The label on the thread** is how the app knows which folder a conversation belongs to.

The label is what survives contact with a scientist. Rename a folder in Explorer, or move one to a
shared drive, and an app that inferred the project *only* from the path is now silently wrong. With
the label it can notice and say so.

### Three decisions, taken by the person who uses it

- **Moving a conversation moves its folder.** What the app shows and what Explorer shows stay the
  same story. Only possible while no turn is running, because the backend holds that path open.
- **Existing conversations stay where they are.** They appear ungrouped; nothing on disk moves on
  first launch. The answer to "what happens to my work" is *nothing*.
- **New conversations inherit the last project used.** You pick once and keep working, rather than
  answering a dialog before every question.

### The mechanism was already there

`overlay/minime_local/workspace.py` already chose a thread's directory from a config key — that is
how a background worker gets pinned to the conversation's folder rather than its own (§43). A
project is the same mechanism with a second key, `__workspace_project__`, so this needed no new
seam, no upstream change, and no route.

### The one genuinely dangerous part, and the test for it

The project name becomes a **path segment**, computed twice: by Python when the backend writes a
turn's outputs, and by Rust when the app goes looking for them. One character of disagreement and a
researcher's figures land somewhere the app will never show them — §89's failure with a longer fuse
and no error anywhere.

It cannot be written once, since it lives in two languages. So it is checked instead:
`the_rust_and_python_project_names_agree` runs both implementations over thirteen names — traversal
attempts, Windows-illegal characters, accents, empty strings, a 200-character name — and asserts
byte equality. That is the same defect shape as §100's scrollbar width in one file and layout in
another, caught before it shipped rather than after.

Sanitising happens on **both** sides rather than trusting the client, because a name is a thing a
person types and `../..` must not write outside the workspace root.

### Where this stands

Built: the folder layout, both sanitisers, the move, and the cross-language test. Still to come:
the sidebar grouping, the "move to project" action, and creating a project — which is now
straightforward, because the part that could quietly lose someone's data is settled.

## 106. Projects, visible (2026-08-07)

§105 laid the ground — the folder layout, both sanitisers, the move, and the test keeping the two
languages byte-identical. This is the half a researcher touches.

**A project is exactly "a name some conversation is filed under".** There is no registry of
projects, because a registry is a second place for the truth to live and the first thing to fall
out of step with the sidebar. The picker derives its list from the conversations themselves, so a
project exists while something is in it and stops existing when nothing is — which is also what a
folder does.

**Creating one is typing its name.** The picker's filter field doubles as the field a new project
is named in: type something no project matches and the top row offers to create it. Choosing an
existing project and creating a new one are the same gesture, so there is no second mode to learn
and no "New project" dialog to find.

### The order the work happens in

`file_in_project` moves the folder **first**, and a failure stops there. Writing the metadata first
and then failing to move would leave the app believing a conversation lives somewhere its files are
not — §89's shape again, and worse here, because from the researcher's side it would look like the
files had been deleted.

It refuses mid-turn, and says why: the backend holds that path open for the length of a turn.

### Three small decisions that are mostly about not being annoying

- **Headings appear only once there is more than one group.** One project is not a grouping; the
  heading would be noise above every row a researcher owns.
- **The order is alphabetical, with "No project" pinned last.** A sidebar that reorders itself as
  work moves is one nobody builds a memory of.
- **Clicking a heading opens the folder.** That is the entire reason a project is a directory
  rather than a label, and it should be one click from the thing that names it.

### What the two halves each do, again

The pairing is easy to lose, so: the **folder** is where the files are; the **label on the thread**
is how the app knows which folder. Two tests hold the ends together —
`a_conversations_project_is_read_from_the_thread_not_the_folder` and
`a_run_names_the_project_folder_its_outputs_belong_in` — on top of §105's cross-language check that
both sanitisers produce the same path segment.

### The four gaps, closed

- ✅ A model per specialist (§104).
- ✅ An About window with the Asta attribution (§103).
- ✅ Projects, as folders (§105, §106).

Untested on a real window: all of it. The next screenshot is the one that matters, and on this
project's record it will find something — §85 and §99 were both settled by one.

## 107. "It says run in a sandbox" (2026-08-07)

Three findings from the first real window on §106. Two were mine; the third was a good idea.

### The About box told every researcher the wrong thing

It read *"Runs in an isolated sandbox"* on a machine running host execution. The check was:

```rust
if self.sidecar.execution() == "local"
```

and the label is `"host (local)"` (`backend.rs:810`). The comparison never matched, so the false
branch was the only one anyone ever saw.

**This is the defect this repo reported upstream against `guardrails.py` the same morning** — a
component telling the researcher their code is sandboxed when it is running on their own
filesystem — reintroduced eight hours later, in the section written to avoid it, by comparing
against a string I assumed instead of read.

§79 had already settled the rule and I did not apply it: *"matching on prose to discover this is
how the two get confused in the first place."* The fix is `runs_locally()`, answered from the
`Execution` enum. There is no string to be wrong about.

Worth stating plainly, because the pattern is now specific rather than general: **every time this
project has compared against a human-readable label, the label has been the thing that changed.**
§79 was `Started::{Attached, Spawned}`. This is the same fix, and the same lesson had a section
number already.

### New thread left the project

Filed under `TEST2`, pressed New, landed in `No project`. `NewThread` consulted
`settings.project`, which is only written when something is *filed* — so opening a conversation
already in a project, then starting a new one, went outside it.

Three changes, and the shape is that "the project you are in" has more than one source:

- **New thread leaves the project alone.** `sidecar.project()` already holds it; overwriting from
  a setting was the bug.
- **Opening a conversation remembers its project**, not just adopting it. Looking at work counts
  as being in it.
- **Startup restores it**, so the first conversation of a morning lands where yesterday's work is.

### A `+` on each heading

Asked for, and right: starting work in a project should not mean starting it somewhere else and
then filing it — which is a folder move for something that had never needed to be anywhere. It
appears on hover, the same way the rename and delete controls on the rows below do.

### The other two held

Per-specialist models worked on a real turn, and the Asta citation copies. The provider tag on
each row — *"Anthropic — no key stored"* — did the job it was added for: it says what is missing
at the moment of choosing, rather than several minutes into a turn that fails inside a subagent.

## 108. Filed at birth, and a spine that belongs to nobody (2026-08-07)

### The `+` put files in the right folder and the row in the wrong place

Pressing `+` on a project heading gave a conversation under **No project**. The project drives two
different things, and §106 wired only one of them:

- `sidecar.project()` → `configurable.__workspace_project__` → **which folder the backend writes
  into**;
- the thread's `minime_project` metadata → **which heading the sidebar shows the row under**.

`file_in_project` sets both. Creating a thread set only the first, because `create_thread` was
written before projects existed and nobody went back to it. So a conversation started from a
project's own `+` had its files in the right place and its row outside — right by one measure and
wrong by the other, which is worse than being wrong by both, because half of it looks like it
worked.

Fixed at the source: `POST /threads` carries the project alongside the conversation tag, so a
conversation is filed at birth rather than needing a move it should never have needed.

**The tell was there in §105 and I wrote it myself**: *"the folder is where the files live; the
label is how the app knows"*. Two facts, deliberately kept separate — and then set in one place
and not the other. Every appearance of this project's oldest shape has been two things that must
agree, and this is the first where I had already written down that they were two.

### The research spine is not per project, and never was

Also reported: the right-hand panel shows completed work from conversations since deleted, and
from other projects. That is not a bug in this app — it is Mini-Me's design, stated in
`backend/runtime.py:141-154`:

```python
def _project_namespace(user_id: str) -> tuple[str, ...]:
    """Namespace for the persistent research-project spine.

    User-scoped as ``(user_id, "project")`` — deliberately **not** keyed by
    assistant_id (unlike memories) …
    * The research project is the user's, spanning every assistant/thread.
```

One spine per person, accumulating forever. That was right when a researcher had one line of work.
With projects it is wrong twice over: it mixes them, and it never forgets.

**Making it per project is not a small change**, because the namespace is computed in two places
that must agree:

- `_project_namespace_for_runtime` (`runtime.py:157`) runs inside a turn and *can* see
  `__workspace_project__` through `get_config()`;
- `routes/project.py:76,100` computes the same namespace in an **HTTP handler**, which has no run
  config — and the docstring says that symmetry is the reason it is keyed the way it is.

So patching the runtime side alone would leave `GET /project` reading a namespace the turns no
longer write to: the panel would go blank rather than become correct.

Three options, none of them free:

1. **Namespace the spine by project, and give the route the project too** — a query parameter the
   client already knows. Correct, and it needs the route to change, which means an upstream
   change rather than an overlay patch.
2. **Stop calling `GET /project` when a project is active** and rely on the spine that arrives in
   each `values` snapshot, which is per-run and would carry the right namespace once the runtime
   side is patched. No upstream change; the panel is empty until the first turn in that project.
3. **Leave it user-wide and say so** — one line in the panel, so it reads as a career summary
   rather than as this project's state.

Not decided here. It is the researcher's call which of "correct but needs upstream", "works now
but starts empty", or "honest about what it is" fits how they work — and this document has a bad
record of choosing a default on someone's behalf and finding out later (§96).

## 109. A spine per project (2026-08-07)

§108 laid out three options for a research panel that mixed every project and never forgot a
deleted conversation. The answer was the first: *"do 1, so we can change it properly."*

Properly, here, means two things at once — because Mini-Me is a pinned, unmodified reference
checkout and the locked decision is *bundled upstream, not forked*:

1. **`overlay/minime_local/spine.py`**, so it works now, with the checkout byte-for-byte upstream.
2. **`docs/upstream/mini-me/project-spine-is-not-per-project.md`**, so it can land at the source,
   where the route can simply take a parameter instead of an overlay smuggling one through a
   `ContextVar`.

That is exactly the shape §18 chose for host execution, and the sentence it used still applies:
*this is the bridge, not a rejection of the PR.*

### Why both halves had to be patched

`_project_namespace` has two callers that must agree: one inside a turn, which can see the run's
`configurable`, and two HTTP handlers, which cannot. Scoping only the runtime side would leave
`GET /project` reading a namespace turns no longer write to — the panel would go **blank** rather
than become correct, which is a worse outcome than the bug.

So the overlay patches the namespace function (covering both callers at once, since both import
it) and wraps the two handlers to read `?project=` into a `ContextVar` the patched function
consults. The client sends it. Backwards compatible by construction: with no project the namespace
is unchanged, so every spine that exists today is what an ungrouped conversation reads.

### Two things that would have failed quietly

**The wrapper had to be `async`.** The handlers are coroutine functions, so a sync wrapper would
set the variable, build the coroutine, reset the token, and return it *unawaited* — the value gone
before the handler ran. Every request would have read the ungrouped spine while the code looked
right. Caught while writing it, which is luck; the shape is §98's exactly, where a test passed
because it exercised a different invocation than production.

**The patch is armed unconditionally.** `install()` returns early unless host execution is on, and
folding the spine patch into that would have tied "which project's spine am I seeing" to "where
does the agent's code run" — two unrelated facts sharing one switch, which is §78 word for word.
The targets are now split: host-execution patches stay conditional, the spine ones always run.

### And the panel is cleared when the project changes

`self.project = None` before each refresh. A stale mission sitting above a new project's empty
list reads as that project having inherited the old one's work — the same reason §79's sidebar had
to say "loading" rather than show nothing.

## 110. The log said which copy (2026-08-07)

Everything §109 asked for showed up in the backend log — the spine per project, both route handlers
wrapped, and the SQLite checkpointer live with its database on ext4 inside the distro, where §95
required it. But two lines further down:

```
Configuring custom checkpointer at
  /mnt/c/Users/LENOVO/Documents/GitHub/mini-me-desktop/overlay/minime_local/checkpointer.py
Importing graph  graph_id=background
  path=/mnt/c/.../overlay/minime_local/async_agents.py
```

**`/mnt/c`.** The background graph and the checkpointer are being imported across WSL's 9p mount
on every launch — the exact dependence §25 removed by provisioning a copy inside the distro, and
§33 later found to have gone stale in the other direction.

### Why, and why it was invisible

`make_config.py` writes **absolute** paths into the generated config, derived from its own
`__file__`. So whichever copy runs it decides where the server imports from for the life of the
process. The launch resolved the overlay two different ways in the same command line:

- `PYTHONPATH` used `overlay_expression`, which probes for the in-distro copy and falls back to
  Windows — correct;
- the generator was handed the raw host path — so it ran from `/mnt/c` and wrote `/mnt/c` into the
  config.

Nothing fails. The imports work, just from the wrong side, and the only evidence is a path inside a
log line. That is the third finding in three days whose whole existence was a value nobody had
looked at: §99's `bounds.size.width`, §91's count of adoptable threads, and now this.

Fixed by handing the generator the same expression, double-quoted rather than `shell_quote`d —
quoting a command substitution as a literal would defeat it, while double quotes keep it running
and still suppress word splitting on a path with a space in it.

### The test is a count

`the_config_generator_runs_from_the_in_distro_overlay` asserts the launch probes for
`sitecustomize.py` **twice** — once for the generator, once for `PYTHONPATH`. Before the fix there
was exactly one. That is a more honest assertion than matching a path, because the thing that was
wrong was not the path's shape; it was that only one of two places asked the question.

### Also from the same log

- `could not adopt older conversations` fired at `warn` on every launch: the adoption pass runs
  before the first conversation list, which happens while the backend is still starting. A warning
  that appears every time is one nobody reads on the day it means something — `debug` now, which
  is what `list_conversations` beside it had already concluded for itself.
- `thread_dir` was left behind when projects gave a conversation's folder a project component.
  Removed.

## 111. Background work was invisible twice over (2026-08-07)

Two findings from one screenshot, and they share a cause: **a background worker runs on its own
LangGraph thread**, so nothing it does reaches the conversation's stream. Everything this app
knows about it comes from the `async_tasks` map in each snapshot.

### The provenance record never heard of it

*"the provenance plots dont capture async agent in the background."* Correct, and it had nothing to
work with: the record is built from stream events, and a background worker emits none here. The
`async_tasks` map was decoded — for the Jobs panel — and never passed on.

It is now, through `observe_background`, which differs from `observe` in one way that matters:
**it searches every turn, not just the one in progress.** Background work outlives its turn. A
theorizer launched in turn three is still in the snapshot during turns four and five, and the
per-turn lookup would file it three times, as three different pieces of work by the same
specialist. The graph would show a node visited three times for one run.

So the deliberate handoffs a researcher makes — the thing `/subagent`'s background mode exists for
— now appear in the record beside the delegations the coordinator makes itself.

### And it was writing to the wrong folder

Found while reading the forwarding code rather than reported. `FORWARDED_CONFIG_KEYS` is an
allowlist of what travels from a conversation's run onto a background run, and projects (§105)
added `__workspace_project__` without adding it there. So a worker pinned to the conversation's
thread — which §43 arranged specifically so its files land where the researcher looks — wrote to
the workspace **root** instead of the project folder. Its report landed outside the project whose
conversation asked for it, while the app looked inside.

The comment sitting directly above that list explains the rule it needed: the thread pin is there
for exactly this reason, and the project is the same kind of key. **A new config value that decides
a path has to be added in two places, and only one of them is where you added it.**

That is this project's oldest shape again — §100's scrollbar width, §108's project-at-birth,
§110's overlay path — and the fourth time in three days. The others were caught by a screenshot, a
log line and a warning. This one was caught by reading a list while looking for something else,
which is the least reliable of the four.

### The empty background results are a separate matter

*"not sure why the background tasks are not working."* Both tasks reported `success` with nothing
useful in the result. That is not diagnosable from this side: the worker's own thread has its own
log, and this app only ever sees the status. What would settle it is the backend log around those
two task ids — whether the worker built a model at all, and what its final state held.

Worth noting the overlay already does the thing that would otherwise be the obvious suspect: it
forwards `model_config`, `__llm_keys` and a recursion limit onto the background run, because
upstream's `start_async_task` sends none (`docs/upstream/mini-me/start-async-task-config.md`). So
the model and key should be there. The empty result is something after that.

## 112. A log line that was wiring, not an event (2026-08-07)

The §110 fix is confirmed in the same log that raised the question:

```
Importing graph  graph_id=background
  path=/home/piero_linux/.local/share/mini-me-desktop/backend/.desktop-overlay/minime_local/async_agents.py
```

In the distro, not `/mnt/c`.

### Why the background log said nothing useful

Three copies of this, and nothing else about background work:

```
minime_local: background work will run on the conversation's own model
  method=GET path=/threads/{thread_id}/state
```

On a **state read**. That looked like the smoking gun — config being captured during a read-only
graph load, where there is no model to capture. It is not: `_forwarded_config()` is called inside
the tool, at the moment a task is launched, and the config is read from the run that is live then.
Checked in the source rather than inferred from the log, which is the only reason this section is
not a fourth wrong diagnosis.

The line was logged where the tool is **wrapped** — which happens on every graph build, including
the read-only ones behind the `GET /threads/{id}/state` the client polls while watching a task. So
it was a wiring step wearing the grammar of an event, at warning level, three times a minute.

### What it says now

Demoted to `info` and reworded to what it is. The line that matters moved into the tool, where a
launch actually happens:

```
minime_local: launching data_voyager with config keys
  ['__is_for_execution__', '__llm_keys', '__workspace_project__', 'model_config'],
  recursion_limit=10000
```

and, when there is nothing to forward, `NONE — the worker will have no model`.

Keys only, never values — one of those is an API key. And it is the **first** thing that would
distinguish the two explanations for the reported symptom: a background run that starts without a
model reports `success` with an empty result, which is indistinguishable from one that ran
properly and found nothing. That ambiguity is §81's, exactly, and it cost four rounds there.

### The task ids were not in the log at all

The grep for the two ids from the transcript matched nothing. Not evidence of anything: the sidecar
log is per launch and the backend had been restarted several times since. Worth stating because a
missing line reads like a finding, and here it only meant the file was younger than the question.

## 113. A wrapper that restated a signature it did not own (2026-08-07)

`install_runtime.<locals>._project_namespace_scoped() takes 1 positional argument but 2 were
given` — and the backend could not start. §109's patch, one launch old.

I wrote the wrapper as `def _project_namespace_scoped(user_id: str)`, matching what
`backend/runtime.py:141` says on **this developer's reference clone**. The checkout a researcher
actually runs is a *pinned* provision of Mini-Me, and there it takes two. So every call into the
research spine raised `TypeError`, on a code path every request touches.

The fix is what the wrapper should always have been:

```python
@functools.wraps(original)
def _project_namespace_scoped(*args, **kwargs):
    base = original(*args, **kwargs)
    project = current_project()
    return (*base, project) if project else base
```

**A wrapper over someone else's function has no business knowing how it is called.** It needs to
pass along whatever it was given and adjust what comes back — nothing more. The same applies to the
route handlers, so they take `*args, **kwargs` too.

Exercised against three arities — one argument, two, and keyword-only — because the whole failure
was an assumption about exactly that, and "it works on the clone I read" is not a test.

### The shape, and it is not a new one

This repo has a rule for it, written down twice and not applied here:

- §79: *"matching on prose to discover this is how the two get confused"* — answer from the type.
- §107: the About box compared `execution()` to a string it had guessed rather than read.
- Here: a signature copied from a file that is **not the one that runs**.

Every one is the same act — treating a local reading of upstream as a fact about the deployed
upstream. The reference checkout at `~/Documents/Mini-Me` is a *developer's* clone with its own
branches; the backend a researcher runs is pinned and provisioned. §110 was the same gap in the
other direction, where the config named `/mnt/c` while `PYTHONPATH` named the distro.

The overlay exists precisely because upstream is not ours to hold still. Code in it should assume
**less** about upstream than code anywhere else in this repo, and this assumed more.

## 114. A worker spawning workers (2026-08-07)

The line that answers the question, from a real launch:

```
launching background_worker with config keys
  ['__is_for_execution__', '__llm_keys', '__workspace_project__',
   '__workspace_thread__', 'model_config'], recursion_limit=10000
  graph_id=agent      ← the coordinator, as intended

launching background_worker with config keys [ …the same… ]
  graph_id=background ← a background worker, launching another one
```

**Every key is there.** The model, the key, the project, the workspace pin, a ten-thousand
superstep budget. So the empty results were never a missing model — which was the obvious suspect
and the one I had said to check first.

The second line is the finding. `graph_id=background` means a background worker executed
`start_async_task` and started a worker of its own. §39 recorded that
`_BUILDING_BACKGROUND` *"still stops a worker spawning workers"*, and on this deployment it does
not. That explains the symptom exactly: a worker asked to do the analysis delegates it onward and
returns `success` with nothing in it, because it did nothing — it handed the work to someone else
and stopped watching.

### Why it could not be seen

`middleware_for` returns `None` when the guard fires, and said nothing either way. So "the guard
worked" and "the guard was bypassed" produced identical evidence: a coordinator that starts.
§81's lesson, for the fifth time in this document, and the first where the silent component was one
I had already written a comment claiming worked.

It now says so, at the moment it declines.

### The second guard

The ContextVar is set around the factory call and read during the build. Whether it survives
depends on the context propagating across every `await` inside `backend.agent.agent` — MCP tool
loading, model resolution, middleware assembly — and on the pinned checkout, evidently, it does
not.

So there is now a second signal that cannot be lost that way: `__is_background__`, set on the
**run's own config** by the launching tool and read back by `building_background()`. Config is the
same channel the model, the key and the workspace already travel on — if it were not reaching the
worker, nothing would work at all.

Two independent sources, either sufficient. That is deliberate: the ContextVar covers builds that
happen inside the factory, the config key covers the run itself, and the failure mode of one is
not the failure mode of the other.

### What this does not yet prove

That the empty results are *only* this. It is a sufficient explanation and it matches the evidence,
but a worker that delegates onward and a worker that runs and returns nothing look the same from
outside — which is the whole reason this took a log line to find. The next run with these two
changes in place will say plainly whether a worker was built with the tool or without it.

## 115. Where did it look? (2026-08-07)

§114's guards held on the next run — one `launching` from `graph_id=agent`, and
`background worker built WITHOUT start_async_task, as intended` from `graph_id=background`. **That
is background work functioning end to end for the first time**, three months after §39 recorded
that it "had never run once."

The next thing it did was fail to find a file the conversation had just written:

> it could not find `/potato_yield.csv` and asked for the exact sandbox-relative path

Two explanations, and from outside they are the same sentence:

1. the worker is looking in the wrong directory — the pin, or the project, did not travel;
2. the file is not where the conversation thinks it is — the *coordinator* wrote it somewhere else.

Both would produce exactly that message. Neither can be ruled out from the transcript.

### The value that decides it was never printed

`LocalWorkspaceBackend.__init__` computes the work directory from three inputs — the run's own
thread, the pin that may override it, and the project — and reported none of them. So it now logs
once per construction:

```
minime_local: workspace /mnt/c/Users/.../Mini-Me/TEST/019fddfc-…
  (own thread 019fde02-…, pinned to 019fddfc-…, project 'TEST')
```

Which settles it in one reading: if the coordinator's line and the worker's line name the same
directory, the pin works and the file is genuinely absent; if they differ, the difference *is* the
bug and the line says which of the three inputs disagreed.

### The fifth time this week

Every argument this week has ended with a value the program already held and did not print:

| | the value | what it settled |
|---|---|---|
| §91 | how many threads a rule would adopt | 1, not 26 — the fix was wrong |
| §99 | `bounds.size.width` | 0.0px — three fixes had missed the mechanism |
| §110 | the overlay path in a log line | `/mnt/c` — imports crossing the 9p mount |
| §114 | the forwarded config keys | all present — the model was never the problem |
| §115 | the resolved work directory | pending |

Four of the five were one line of logging. The pattern is specific enough now to act on in advance:
**when a component computes something from several inputs and then behaves surprisingly, print the
thing it computed, not the inputs.** Every one of these was a derived value, and in every case the
inputs looked fine.

## 116. The pin works (2026-08-07)

§115's line, from the background worker's own run:

```
minime_local: workspace /mnt/c/Users/LENOVO/Documents/Mini-Me/test2/019fdd9e-e0cd-…
  (own thread 019fde06-e33c-…, pinned to 019fdd9e-e0cd-…, project 'test2')
  graph_id=background
```

Its own thread is `019fde06-…`; it resolved to the **conversation's** `019fdd9e-…`, inside
`test2`. Both §43's thread pin and §111's project key travelled onto a background run, and the
coordinator's own turns name the same directory.

So of §115's two explanations, it is the second: **the worker looked in the right place and the
file was not there.** Which points the enquiry at the foreground turn that was supposed to write
it — a completely different question from the one that was being asked, and one nobody could have
reached from the transcript.

Three sections of instrumentation to establish that nothing was wrong. That is a fair price:
§114's guard bug and §111's missing config key were both found on the way, and neither would have
surfaced without it.

### And the log now knows what is worth saying

The same run produced a dozen of these:

```
workspace .../Mini-Me/019fde06-… (own thread 019fde06-…, pinned to 019fde06-…, project '<none>')
  method=GET path=/threads/{thread_id}/state
```

Read-only graph loads. `GET /threads/{id}/state` builds a backend too — the client polls it every
few seconds while watching a task — and those have no run config, so they resolve to the run's own
thread at the root and touch nothing. At warning level they outnumbered the lines that mattered six
to one.

`warning` when there is a live run, `debug` otherwise. **A log that reports everything is a log
nobody reads**, and this project has now made that mistake twice in one day — §112's wrapper
message was the other.

The rule that falls out, and it is the counterpart to §115's: *print the derived value, at the
moment something derives it for a reason.* Not on every construction, and not only when it fails.

## 117. Proposed — outputs a turn wrote into a folder (2026-08-07)

**Background work is done.** The worker found `potato_yield.csv` in the conversation's own
directory, profiled it, and produced eight tables and eight plots. §39 recorded that background
work "had never run once"; it now runs, on the researcher's model, in the right folder, without
spawning workers of its own.

And the moment it worked, it exposed the next thing:

> Files created: `./hola_eda_outputs/dataset_summary.csv`,
> `./hola_eda_outputs/yield_by_clone_boxplot.png`, … *(sixteen of them)*

The Outputs panel showed two files. `provenance.json` and `potato_yield.csv` — the ones at the top
level. **Every artefact of the analysis was invisible.**

### The cause is one level deep, and it is not about background work

`workspace::outputs` and `workspace::images` both call `read_dir` and then `is_file()`, so a
directory is not descended into — it is dropped. Any turn that organises its output into a folder
disappears from the app, and organising output into a folder is what analysis tooling does. A
*foreground* EDA has exactly the same problem; the background worker only made it obvious, because
naming an output directory is the first thing that specialist does.

This is §42's argument for the third time. Outputs were moved to `Documents\Mini-Me` because
*files a researcher cannot find are files that do not exist*; projects became folders for the same
reason (§105); and now the app cannot see one level down from its own workspace.

### What a fix has to decide

Not difficult, but not a one-line recursion either — four judgement calls:

- **How deep, and how many.** A turn can write a virtualenv, a cache, or a dataset tree of ten
  thousand files. A bounded walk (a few levels, a capped count) with the cap *stated* when it
  bites, rather than a silent truncation — §51's rule.
- **What to skip.** `__pycache__`, `.ipynb_checkpoints`, anything dotted. The existing top-level
  reader already drops dotfiles as "the agent's business, not the researcher's"; the same judgement
  extends downward.
- **How to group.** `hola_eda_outputs/` is a *meaningful* name the agent chose — the panel should
  probably show the folder as the grouping rather than flattening sixteen files into one list. The
  Kind buckets (Data, Figures, Reports) may want to become secondary to it.
- **What `collect_plots` does with it.** §42 attaches new figures to the newest answer by diffing
  the workspace. Recursing changes what that diff sees, and a turn that writes forty plots into a
  folder would flood the transcript. Probably: the panel recurses, the transcript keeps a
  conservative cap and says how many it did not show.

### Why it is worth doing properly rather than quickly

The panel is where a researcher goes to find what a turn produced, and it currently says *"Papers,
datasets, theories and reports show up here as a turn produces them."* That sentence is a claim the
code stopped making true the moment an agent organised its own work — and this document's most
expensive defects have all been claims that quietly stopped being true (§107's sandbox line,
§82's docstring, §72's stated width).

Sequenced after the release, not before: it changes what the panel shows, and that is worth
watching on a real conversation rather than shipping blind.

## 118. The designer's brief, and the four places it could not be followed (2026-08-07)

A design handoff arrived as a zip: a README written against this codebase — GPUI logical pixels,
`p_4`/`text_xs` helpers, the exact `theme.rs` field names, and which function in `main.rs` each
change touches — plus an HTML canvas of frames to look at rather than port. The instruction with it
was specific: implement the `5a`/`5b` palette and the `2a`–`2d` frames; `0a`–`0h` are the current
UI recreated as a baseline and `3a`–`4d` are rejected explorations kept for context.

Implemented over seven commits: the **Bench** palette and the accent discipline that goes with it,
the **road strip**, the **research panel**, the **empty state**, **provenance chips and an export
row**, the **approval card**, **inline output cards**, and the **provenance graph**.

### The claim that was checked before it was built on

The README said of the two palettes: *"The two existing tests cover them unchanged — every
ink/surface pair clears 4.5:1 and luminance rises across the ladder."* Running the numbers before
writing any of it: **six of the pairs do not.** `text_faint` on Bench's background is 4.41:1,
`running` 4.45:1, both again on `accent_soft`; Bench Night's `error` is 4.44:1 on `overlay` and
4.13:1 on `accent_soft`. The floor `every_shipped_theme_is_readable` enforces is 4.5.

Hue and saturation were kept; only lightness moved, by two or three points per channel — the
smallest change that clears AA. Had this been taken on trust the theme would have failed its own
test on first run, and the obvious repair under time pressure is to relax the test.

**The rule, and it is not new here:** *a number in a handoff is a claim, and the cheapest ones are
worth checking before they become the foundation.* This is §72 and §99 again, from the other
direction: there the app stated a width nothing had measured; here a document stated a ratio
nothing had computed.

### Four things the design drew that the data cannot support

Each of these is a place where following the drawing would have meant showing a researcher
something the program does not know. They are listed together because they are one decision made
four times, and it is §73's: *a record that quietly guesses is worse than no record, because it
will be believed.*

| Drawn | Built | Why |
|---|---|---|
| A dashed **"anticipated"** road node | Two states only | Nothing carries a plan. `Snapshot` has buckets, jobs, tasks, reports and sources; the coordinator decides its next delegation while answering. A dashed `analyze data` under a running `get data` is an invented plan shown as a record. |
| `6 pages · 8 references` on a PDF | Size alone | Page counts need a parser or one of the folklore heuristics (`/Type /Page` double-counts and misses object streams; `/Count` finds the first of several). Reference counts are not in the file in any recoverable form. |
| The **specialist** that asked for approval | The tool | `ApprovalRequest` carries no subagent. It could be inferred from whichever spoke most recently — very likely right, and an inference stated as fact beside a security decision. |
| `today 14:22` on a resume card | `2 hours ago` | Local wall-clock needs a timezone database. Relative time is exact in every timezone and needs no table. |

Two more were substitutions rather than omissions. **Save as PNG became Save as SVG**: rasterising
needs a screenshot API gpui 0.2.2 does not expose, or a hand-written encoder, on a build that has
to succeed on a colleague's Windows machine with nothing installed — and a vector figure is what a
journal wants anyway. **Copy BibTeX emits `@misc` with the citation verbatim in `note`**, because a
source is one line of the agent's prose and a parser for that is right about most citations and
confidently wrong about the rest; a mis-split reference does not look broken in a manuscript, it
looks like a citation with the wrong author on it.

And one place the design was **not** followed for a safety reason: §4 shows three actions on the
approval card, dropping *"Approve the rest of this turn"*. Both grants were kept and moved right at
`Compact`. Removing the narrower one leaves "approve everything in this conversation" as the only
way to stop clicking, which is how a gate becomes a formality — §41's argument, unchanged.

### What the brief was right about that the code was not

Three of its observations were about facts the codebase had let drift, and each was a real defect
rather than a matter of taste:

- **Headings wearing the accent.** `section_label`, `section_label_owned` and the modal title all
  used `theme::accent()`, against `theme.rs`'s own first documented rule — *"the accent means 'you
  can act on this', and nothing else"*. The module said it, the tests did not check it, and three
  functions broke it. One line each.
- **The default named in three places.** `Settings::default().theme`, `apply_theme`'s fallback and
  the `live_theme!` seeds each restated the default palette. Changing "the default" therefore meant
  changing three things that had no way to disagree out loud. `THEMES[0]` now says it and the other
  three read it — the §91/§114 shape once more.
- **The §117 placeholder.** *"Papers, datasets, theories and reports show up here as a turn
  produces them"* is deleted. It was untrue, §117 says why, and an empty section now renders
  nothing at all.

The panel states this correctly now, but **§117 itself is still open**: `workspace::outputs` reads
one level, so a turn that organises its work into a folder still has files the app cannot see. What
changed is that the app no longer promises otherwise.

### The distinction that was in a comment and not in the data

Building the graph (`2d`) turned up the sharpest instance of this document's recurring bug. The
design asks for four line styles, one of them for an edge crossing a turn boundary. `provenance.rs`
computed that edge separately and had done since §75 — its comment reads *"the turn boundary needs
no hedge: the researcher read one answer before typing the next question, so this ordering is a
fact about a person, not an inference about a scheduler"* — and then filed it as `Edge::Then`,
alongside the hedged within-turn case.

So the view drew them identically, and the returns §73 asked the whole feature to make visible were
the one thing it could not point at. `Edge::Returned` is the missing variant; the reasoning was
already written down, in prose, one type short of being real.

*A distinction that lives only in a comment is a distinction the program does not make.*

### Where this leaves the release

Everything in the brief's implement list is done, 208 tests pass, and clippy is at the seven
warnings that predate this work. What has not happened is a **real-window pass**: every screen here
was built against the README's measurements and the existing code, not looked at on Windows at
125% scaling — which is where §72, §85, §88 and §99 were all caught. That is the next step, and it
is the researcher's, not the developer's.

## 119. Five references, and none of the DOIs were real (2026-08-07)

Reported from a real run: *"I noticed that the doi links are wrong. Asta search on semantic
scholar but not sure why the doi are not redirecting to the correct papers."*

### What was checked, and against what

First against Semantic Scholar, then — after the reasonable objection that **we** might be using
the API wrongly, or that "these papers have a unique ID" — against **Crossref**, which is the DOI
registrar rather than an index. That second check is the one that counts, and it was the right
challenge to make: an S2 404 proves nothing about a book chapter or a 1997 journal article, and
two of the five conclusions had been drawn from exactly that.

Crossref agreed with Semantic Scholar on every one. S2 was not at fault.

| The citation claims | The DOI is registered to |
|---|---|
| Lindqvist-Kreuze & Forbes 2018, ch. 14, pp. 467-486 | *Gender Topics on Potato Research and Development*, Mudege et al., pp. 475-506 — right book, wrong chapter |
| Hijmans & Spooner 2001, AJB 88(11), 2101-2112 | *Algal switching among lichen symbioses*, AJB 88(8) |
| Vargas et al. 2012, AJPR 89(6), 444-453 | *Resistance to Aphids, Late Blight and Viruses…*, Davis et al., AJPR 89(6), 489-500 |
| Douches et al. 1997, Potato Research 40(4) | **not registered**, and no such title in Crossref |
| Ellis et al. 2018, Euphytica 214 | **not registered**, and no such title in Crossref |

### The mechanism, from the one case that pins it exactly

Hijmans & Spooner 2001 is a real paper, and the model got **volume 88, issue 11, pages
2101-2112** — all correct. Its DOI is `10.2307/3558435`. The model wrote `…3558457`, which is a
real DOI, in the same journal, in the same year, belonging to a study of lichens. Vargas is the
same shape: the model said AJPR 89(6), and the DOI it produced genuinely is AJPR 89(6) — a
different article in that issue.

So every field a person sanity-checks — journal, year, volume, pages — comes out right. A DOI
suffix is a high-entropy string carrying no meaning, which makes it the first thing a language
model loses and the last thing a reader can catch by eye. **That asymmetry is the entire argument
for checking it in software rather than telling people to be careful.**

### Where the client was complicit

`AcademicSourceFinding.citation` (`backend/schemas.py:31`) is a Pydantic field the *model* fills.
The stable link is a **separate** field — `SourceArtifactPayload.link`, and for a theory's papers
`PaperRefPayload.url`/`.doi`, built by `theory_tools.py:_paper_ref` straight from
`s2Metadata.externalIds.DOI`.

`decode_sources` read `citation` and dropped all three. Every link in the app was regexed out of
the model's prose while the identifier the API returned sat one key away. `grep -n '"link"\|"url"\|"doi"'`
over `protocol.rs` returned nothing.

`_paper_ref` carries a comment recording that an S2 URL form *"resolves UNRELIABLY (it sent users
to the wrong paper)"*. Somebody upstream had already paid for this exact mistake and fixed it on
their side; we reintroduced it on ours.

**Fourth instance of the recurring shape** (§91, §99, §115): *a value the program already had and
never read.*

### The attribution was unearned

`used_asta` was `len(sources) > 0` — the number of citation objects the model emitted — and it
controls a footer reading *"Academic literature search performed using Asta tools (Allen Institute
for AI). Please cite the AstaBench paper."*

On this run that footer would have credited AI2 for five references their tools never returned.
Attribution is a claim about provenance, so it now comes from the provenance record: an
Asta-backed specialist must actually have run. Which specialists those are is read from
`subagents.json` — three describe themselves that way — rather than listed in the client, which is
what §55 built that file to prevent. An unreadable registry credits nobody: a missing
acknowledgement can be added, a false one has to be retracted.

### What now exists

A **Check DOIs** button in the sources panel. Each DOI goes to Crossref, the returned title is
compared against the citation *here*, and every reference gets a verdict. Verified against live
Crossref: the model's Hijmans DOI scores 0.00, the correct one 1.00, Douches comes back
unregistered.

Three rules it is built to:

- **A DOI leaves the machine and nothing else** — not the citation, not the question. The
  user-agent names the app and carries no contact address, because that would be the researcher's
  own email going to a third party on every reference.
- **`Unreachable` is not a verdict about a reference.** Telling somebody on a train that a
  citation is unregistered would have them delete one that was fine.
- **A reference with no identifier is reported**, not left blank looking like one that passed. In
  a run where the model wrote its own citations, that is the strongest signal of the three.

### What none of this fixes

If the model invented the paper, a structured `link` can be invented alongside it — on the
academic-research path that field is model-filled too. Only `_paper_ref` is built from real API
metadata. The open question is whether the literature search ran at all, and the provenance graph
built in §118 is what answers it: if no search specialist appears in the record, nothing was
searched. Worth checking on the run that produced these five.

*Two rules, and the second is the one that generalises: **an attribution is a claim, so it should
be derived from the record and not from a proxy** — and **the fields a reader can check are the
ones a model gets right.***

## 120. Asta returns a corpus id, and we asked the model for a DOI (2026-08-07)

§119 left one question open: *why* are the DOIs wrong. The answer came from the researcher, not
from me — they noticed that Semantic Scholar shows a **Corpus ID** where our citations show a DOI,
and said: *"Asta mcp when search papers I think return the corpus ID so we must check if we are
using it well."*

That was the whole thing.

### What the tool actually returns

```
$ asta papers snippet-search "late blight resistance Andean potato landraces" --limit 2
paper keys: ['authors', 'corpusId', 'openAccessInfo', 'title']
```

A title, an author list, and a numeric `corpusId`. **No DOI, no year, no venue, no volume, no
pages.**

And `AcademicSourceFinding.citation` asks the model for *"APA-style or equivalent citation"* —
which needs every one of those. So the model is handed a paper it cannot cite and asked to cite
it. It fills the gaps from memory, which is the only move available.

This is not a model behaving badly. **It is being asked for data it was never given.**

### Why the failure looked the way it did

The Plaisted case pins it exactly. Our citation said *American Potato Journal, 66, 603–627* — both
correct — and gave `10.1007/BF02853934`. The real DOI is `BF02853982`. The model reconstructed the
volume and pages accurately from the title and authors it *was* given, and then produced a DOI,
which is a high-entropy string with no meaning in it and therefore the one field that cannot be
reconstructed.

So every field a reader checks by eye comes out right, and the one nobody checks is wrong. In
three of six cases it resolves, so the link works and opens a real paper on a related subject.

### The fix already existed, forty lines away

`theory_tools.py:_paper_ref` handles this for the theorizer path, with a comment that reads like a
scar:

> Theorizer papers usually carry ONLY a numeric corpusId (no DOI/url). … the website's
> `/paper/CorpusID:<n>` path resolves UNRELIABLY (**it sent users to the wrong paper**). The API
> endpoint `api.semanticscholar.org/CorpusID:<n>` 302-redirects to the correct canonical paper
> page — verified across ids — so link through that instead.

Somebody met this, worked out that a corpus id is all that arrives, established which URL form
resolves, and wrote it down. The academic-research path has no equivalent: `corpusId` never
reaches `SourceArtifactPayload` at all. Written up as
`docs/upstream/mini-me/academic-sources-drop-the-corpus-id.md`.

### What was built on this side

**Find the right DOI.** The citation still contains a real title — that much *did* come from the
search — so Crossref's `query.bibliographic` can be given the whole reference and asked which
registered work it describes. On the real Plaisted citation, including its invented title wording,
it returns the correct paper at 0.75.

The runner-up scored **0.57** against a 0.6 floor: *Solanum amayanum: A new wild Peruvian potato
species*, which shares "wild", "potato" and "Solanum" with the model's invented title. Six points
of separation is not grounds for telling a researcher which paper they meant, so a repair must also
beat the next candidate by 0.15.

That threshold is the interesting decision. Verifying a DOI answers *"is this the paper"* about a
work the citation already named. Repairing **picks** one and says *"this is it"* — a stronger
claim, and one made with the app's authority rather than the model's. A near-tie there is not a
weak yes; it is precisely the case where answering reproduces the bug being fixed.

*Two plausible answers is not an answer.*

### The shape, stated

Three of the last five defects in this document are the same one. §119 was a value the program had
and never read. §118 was a distinction that lived in a comment and never in the data. This one is
**a field asked of a component that was never given the data to fill it** — and the tell, in every
case, is that the wrong answer is well-formed. A fabricated DOI parses. An `Edge::Then` that should
have been `Returned` draws fine. A regexed link opens something.

*Nothing about a plausible answer tells you where it came from. Only the record does — which is
why the provenance work and the citation work turned out to be the same project.*

### §120a — a second failure, hiding behind the first (2026-08-07)

Testing the repair turned up a reference of a different kind:

> Sørensen, K. J., Kirk, H. G., & Poulsen, K. (2006). Use of Andean potato landrace populations to
> identify new sources of resistance to late blight. Euphytica, 152(3), 305–316.

DOI unregistered. Title search: `total: 0`. No such paper in Semantic Scholar or Crossref, under
the title or the authors. Douches 1997 and Ellis 2018 from §119 are the same.

So the six references are **two defects**, not one:

| | | corpus id fixes it? |
|---|---|---|
| Plaisted, Hijmans, Vargas, Lindqvist-Kreuze | real papers, invented identifiers | yes — §120 |
| Sørensen, Douches, Ellis | no such paper, invented whole | **no** |

The researcher's instinct — *"I don't care to have a DOI, we can have the corpusid url"* — is
right for the first class and cannot touch the second. A CorpusID URL redirects to a paper, and
there is no paper.

**The reason this matters beyond the fix:** the second defect was invisible while the first
existed. Every reference had a wrong DOI, so "wrong DOI" was the explanation for all six, and it
was the correct explanation for four. Fixing the identifiers would have made the app produce four
good citations and two that resolve to nothing — and that would have looked like a regression in
the fix rather than a defect it uncovered.

*A defect that explains every instance is not thereby the only defect. It is a reason the others
have not been noticed.*

### What the repair now says

The lookup refusing to answer was correct — nothing matched, and §120's margin rule exists
precisely so it stays quiet rather than naming the closest paper. But a red flag followed by
silence is not a usable result, and the researcher's report was *"its not working"*, which is fair.

`repaired` now records `None` as a finding rather than as an absence: **no registered work matches
this reference — it does not appear to describe a real paper.** That is the strongest statement
this feature can make, and it is more useful than a corrected DOI, because the answer is not
"cite it differently" but "do not cite it".

The same distinction that has run through §118–§120: *asked and found nothing* and *never asked*
must not look the same on screen.

### §120b — the negative was over-claimed (2026-08-07)

The researcher, on being told Sørensen 2006 does not exist: *"Sorensen exists as a book not a
paper. there is a betdiversity index by sorensen btw."*

Both halves land, and the second is the sharper one.

**Sørensen's 1948 similarity index** — the basis of the beta-diversity coefficient, one of the most
cited works in plant ecology — is a monograph in *Biologiske Skrifter*, and Semantic Scholar has it
with **no DOI at all**. Crossref registers journal articles; books, monographs, society series and
grey literature are largely not in it, and older works generally are not.

So `total: 0` is a fact about an index, not about the world — and §120a stated it as though it
were about the world. The row read:

> no registered work matches this reference — **it does not appear to describe a real paper**

Told to a researcher who had correctly cited a monograph, that is worse than the fabrications the
feature exists to catch, because it arrives with the app's authority instead of the model's. It now
reads *"nothing in Crossref matches this — which covers journal articles, so a book or a monograph
may not be there. Check it by hand."* Amber rather than red: something is wrong with the reference,
and *which* thing is not established.

**And the authors are real.** Searching them returns *Linkage and quantitative trait locus mapping
of foliage late blight…* (2006) — **K. Sørensen, M. Madsen, H. Kirk**, D. K. Madsen. Right people,
right year, right subject, different title. Not a reference conjured from nothing: a real research
group with someone else's title and identifier attached to them, which is harder to catch than pure
invention.

### This is the same mistake, three times, from three directions

| | the over-claim | what actually held |
|---|---|---|
| §119 | "these DOIs don't exist" — from a Semantic Scholar 404 | Crossref, the registrar, was needed to say it |
| §120a | "this reference describes no real paper" — from `total: 0` | the index does not cover books |
| — | the check's own copy | now says what was checked, not what was concluded |

Each time the correction came from the researcher, and each time the pattern was identical:
**a negative from one source, reported as a fact about the world.** The feature built to stop a
model from over-claiming was itself over-claiming, in its error path, where nobody looks.

*Report what was checked. A tool that says more than it verified is the thing it was built to
replace.*

## 121. The corpus id, put where it was always available (2026-08-07)

> *"I dont care to have a doi url in the front end. If asta give a corpusId, we can put that ID
> into an url from semanthic scholar and we can be redirected to semanthic scholar. Thats what I
> want."*

§120 established why the DOIs are wrong — Asta returns `corpusId`, `title` and `authors`, and
`AcademicSourceFinding.citation` asks the model for an APA citation, which needs five fields it
was never given. The report went upstream. This is the part that did not have to wait.

### Where it goes in

`overlay/minime_local/sources.py`, through the §18 import hook, in two places:

- **`backend.mcp_tools`** — patches `_wrap_mcp_tools` and lets the original run *afterwards*, so
  the recorder ends up **inside** upstream's capping wrapper. That ordering is the whole trick:
  above it we would see the truncated result, or the 2 KB preview left behind when a large result
  is written to the sandbox — and `mcp_tools.py:132` puts the `asta` threshold at 32 KB while its
  own comment says paper searches run to hundreds of KB. The ids are in the part that gets cut.
- **`backend.middleware.artifacts`** — wraps `ArtifactCaptureMiddleware.after_agent`, which is
  where a subagent's structured output becomes the `sources` list the client reads, and replaces
  `link` with `https://api.semanticscholar.org/CorpusID:<n>`.

That URL form and not the website's, because `theory_tools.py:_paper_ref` already settled it: the
`/paper/CorpusID:<n>` path *"resolves UNRELIABLY (it sent users to the wrong paper)"*. Somebody
paid for that once and wrote it down.

### Why capture at the tool rather than reconstruct later

Because at the tool the identifier is **known**, and everywhere after it is a guess. The client's
"Find the right DOI" repair takes the title out of a citation and searches for it, and carries
every uncertainty a search has: near-matches, ties, an index that does not cover books (§120b).
Here the corpus id is sitting in the response the model is reading. Nothing needs inferring.

That difference now has a name in the client. `Verdict::FromSearch` is **not** a weaker answer than
`Confirmed` — it is a stronger one. A DOI has to be verified because the model wrote it; a corpus
link cannot name the wrong paper for the same reason a file path cannot, because nothing composed
it. So a source carrying one is settled with no registry call at all.

### One rule, two languages, checked

The matcher — same noise words, same 0.6 threshold, same 0.15 margin — is now written in Python
(which citation does this corpus id belong to) and in Rust (which registry record does this
citation name). `the_rust_and_python_matchers_agree` runs both over the real §119/§120 cases and
compares, the same discipline as `the_rust_and_python_project_names_agree` and for the same reason:
two implementations of one rule is a shape this project has got wrong before, and here the failure
would be the backend and the client silently disagreeing about which paper a citation names — in
the one feature built to stop exactly that.

### What is still not fixed

A citation whose title matches nothing the search returned gets no link, because there is nothing
to link it to. That is §120a's second class, it is the subagent citing papers it was not given, and
no amount of identifier plumbing reaches it. It stays in the upstream report as its own
recommendation.

*The fix was never a lookup. It was carrying a value forty lines further than it had been carried.*

## 122. The buttons were the bug (2026-08-07)

> *"Sorry but having these button is dumb. Why the user must check something we should do for them
> before put that into the ui? … I told you I dont want to show the doi link just the word link and
> when I press it I am redirected to the paper in semantic scholar."*

Correct on all three counts, and the first one is a design error rather than a preference.

### Why a button was the wrong shape

**Check DOIs** asked the researcher to request a check on data the app had already decided to show
them. That is the wrong way round: either a citation is worth verifying, in which case verify it,
or it is not, in which case do not offer to. And it asked for work only the app can do — a network
call per reference and a title comparison — whose answer is the same every time it is asked.

**Find the right DOI** was worse: a *second* button, to learn which paper a citation meant, after
the first had already established that the one written down was wrong. Two clicks to be told
something the app knew how to find out on its own.

Both are gone. Verification runs in the background as sources arrive, only for citations not
already answered, and says nothing when everything checks out — a line under each of fourteen
references confirming it is fine buries the two that are not.

*A control that asks the user to authorise work they cannot do themselves, on a question with one
right answer, is not a choice. It is unfinished work with a button in front of it.*

### The link is the word "link"

The row showed a full DOI URL, which wraps mid-token in a 330px column and is not information
anyone wants to read. It now shows **link**, and it points at the paper's page on Semantic Scholar
— not the publisher's landing page, which is a paywall as often as not.

`api.semanticscholar.org/<id>` 301-redirects for both id forms, verified live:

```
CorpusID:45447591                  → /paper/117e16e7774ff0616b461a075feadcee7a33d793
DOI:10.1016/0304-3878(92)90044-a   → /paper/bbec167725ba916adafcaa221f934b759e2cd131
```

So the link is always a Semantic Scholar link, whichever identifier survives: the corpus id the
search returned (§121), the DOI the registry says the citation describes, or the DOI it carried
when that one checked out. Only a source with none of the three — a thesis in a university
repository — keeps its own URL, because a working link to the right document beats a Semantic
Scholar page that does not exist.

### The disclosure has to be stated, not clicked

Automatic means nobody is choosing per use, so the module now says exactly what goes out: a DOI for
every reference, and the citation text for one whose DOI is wrong or missing — because that text is
the query that finds the real work. To `crossref.org` and nowhere else. Never the question, never
the conversation, never a file.

That is the trade the button was standing in for, and stating it once in the code is more honest
than making somebody re-consent every time they want a reference checked.

## 123. The isolation that isolated the wrong thing (2026-08-07)

Asked directly whether the Asta literature problem is solved. Checking before answering found that
**§121's overlay would have done nothing at all.**

`sources.py` kept its `{title: corpus id}` store in a `ContextVar`, reasoned as *"so two concurrent
turns cannot read each other's papers"*. Measured:

```python
await asyncio.create_task(tool_call())   # records one paper
len(_papers())                           # 0
```

A `ContextVar` set inside a child task is invisible to the parent — copy on write, one direction —
and LangGraph runs a tool call in a task while the middleware that reads the store runs outside it.
So every source would have kept the model's invented link, silently, and the success log would have
printed `0 of 6 sources carry the corpus id`.

That is **§114 exactly**: an isolation mechanism that isolated the wrong thing, written by me,
one day after documenting §114. It was never run against a live backend — the unit tests exercised
`remember` and `link_for` in one context, which is the only arrangement where it works.

The store is now process-global and bounded. The isolation was not merely broken, it was
unnecessary: the match is on the **title**, so a citation only takes a corpus id when it names that
paper, and a paper named in one conversation is the same paper when named in another. Sharing can
only produce the right answer sooner.

*A test that exercises a function in the arrangement the author imagined is not a test of the
arrangement the program uses.*

### The MCP is not the limit — the tool choice is

Compared, on one query:

```
MCP  snippet-search   → authors, corpusId, openAccessInfo, title
CLI  asta papers      → authors, externalIds{DOI}, paperId, publicationDate, title, venue, year
```

Semantic Scholar holds the year, the venue and the DOI; `snippet_search` does not return them
because it exists to return *passages*, with just enough paper attached to identify one. It is
being used as the sole source for a task that needs bibliography.

So `academic_researcher` has no tool that returns a year, a venue or a DOI, and is asked for all
three. Added to the upstream report: the fix is a paper-lookup tool beside the snippet search, or a
resolution step from `corpusId` — not more plumbing downstream of a tool that was never meant to
answer this.

## 124. Seven tools, and nothing that says so (2026-08-07)

*"Ok and to be clear, what are the capabilities of the subagents that search papers? Only search?"*

No — and answering it properly turned up why the citations are wrong, which is not what §120 said.

### What `academic_researcher` actually holds

Its declaration is `"tools": []` (`backend/subagents.py:50`). Everything arrives at runtime:

* the **entire, unfiltered** Asta MCP bundle — `get_mcp_tools(("asta",))` with no allowlist
  (`backend/mcp_tools.py:413-414`), against the Dataverse loader which whitelists three by name;
* the **full deepagents filesystem toolkit** — `ls`, `read_file`, `write_file`, `edit_file`,
  `glob`, `grep`, `execute` — prepended to *every* subagent (`deepagents/graph.py:547-560`,
  `middleware/filesystem.py:789-797`).

It cannot delegate: `task` belongs to the main agent alone (`deepagents/graph.py:683-695`). And it
runs **outside the guardrail stack** — PII redaction and the model/tool-call limits are applied to
the coordinator only (`backend/agent.py:124-143`) — while holding `execute`. Whether our own
approval patch catches that combination is not established and is worth checking on its own.

### The reason the DOIs are wrong is not the one §120 gave

§120 concluded the model invents identifiers because `snippet_search` returns none. True, but
incomplete: **it has six other tools that do**, and has had them all along.

`skills/research/SKILL.md:69-82` names all seven by purpose, including which returns full metadata.
The subagent almost certainly never reads it. Every subagent declares its skill one directory too
deep:

```
on disk        skills/research/SKILL.md            SKILL.md is a file inside research/
coordinator    skills=["/skills/"]                 scans subdirs → finds twelve ✓
subagent       "skills": ["/skills/research/"]     scans subdirs of research/ → finds none ✗
```

The loader wants a path whose *subdirectories* hold a `SKILL.md`
(`deepagents/middleware/skills.py:749-762`); the prompt then renders *"(No skills available
yet…)"*. A child cannot inherit the parent's either — `skills_metadata` is stripped from the state
passed down (`middleware/subagents.py:186-192`). **All ten subagents have the same path shape.**

So the picture is complete: seven tools, no document explaining them, and an instruction to produce
APA references. It reaches for the one tool whose purpose is guessable from its name and writes the
rest from memory. Filed as `docs/upstream/mini-me/subagent-skills-point-one-level-too-deep.md`.

### The cheap experiment, before the expensive one

The obvious remedy — a code-side search tool over the `asta` CLI — is a permanent fork: our own
tool definitions, parsing, and CLI version drift, re-checked on every upstream update. It is also
premature, because the subagent already *has* the tools it needs.

So the overlay appends to `academic_researcher`'s prompt instead: name the metadata tools, say what
`snippet_search` does not return, and forbid the three inventions — never write a DOI from memory,
never fill a year or a volume the tools did not give, **cite only papers a tool returned in this
conversation.** That last one is the first thing aimed at §120a's fabricated references, which no
amount of identifier plumbing could reach.

Appended, not replaced: upstream's prompt sets the role and the source limit, and rewriting it here
would silently drop whatever upstream adds next.

If the identifiers come out right, the code-side tool is unnecessary. If they do not, that is
evidence rather than a guess — which is the only reason to build the expensive thing.

*Three explanations for one defect, in three days: the client dropped the field (§119), the tool
does not return it (§120), the document naming the tool that does was never delivered (§124). Each
was true. Only the last one was the cause.*

## 125. The installer said yes and did nothing (2026-08-07)

The prompt patch and the artifact patch fired on the first real run. The recorder did not:

```
minime_local: no _wrap_mcp_tools — sources keep the model's own links
minime_local: academic sources link through Semantic Scholar
minime_local: academic_researcher told which tools return identifiers
```

`_wrap_mcp_tools` does not exist and never did. The real name is `_make_mcp_tools_resilient`
(`backend/mcp_tools.py:351`). I had read that function's *body* while working out where to hook,
and then named it from memory — **§113 exactly**, the wrapper that assumed something about code it
does not own, and the second time this week.

### The second mistake was worse than the first

The fix looked up the right name through a candidate list — and then assigned the wrapper back to
`module._wrap_mcp_tools`, the name that does not exist. So `_recording` was stored under an
attribute nothing calls, `_make_mcp_tools_resilient` was left untouched, and the installer logged:

```
minime_local: recording the corpus id of every paper Asta returns
```

A success line, for an installation that had installed nothing. That is a worse failure than the
original, because the original *told the truth*.

Caught only by driving the real chain — upstream's wrapper around ours, the tool call in a child
task — and asserting on the store afterwards. The unit tests had exercised `observe` and
`link_for`, both of which were correct throughout.

*A rename fixed in one of its two places is a rename not fixed.*

### What the logging earned

Three §81 arguments paid off in one run:

* the installer that speaks **on success** is why the missing hook was noticed at all — the two
  that worked and the one that did not were distinguishable at a glance;
* the failure line now reports **what is actually in the module**, not merely that the guessed name
  was absent. The first version named the guess and not the fact, leaving the answer in the file it
  had just failed to read;
* the success line now names the hook it installed (`via _make_mcp_tools_resilient`), so "installed
  under the wrong name" can never again look like "installed".

### Still unproven

The chain is verified against upstream's real function name, its real wrapping order, and a child
task — but not yet against a live backend. The number to look for is
`minime_local: N of M sources carry the corpus id Asta returned`. Until that reads something other
than `0 of M`, this remains a patch that has passed a rehearsal.

## 126. Seventeen papers, checked against the registry (2026-08-07)

*"Test at least 15 papers from different themes to verify it works. You test must compare the
builded doi and visit the doi link to check if its correct what we see in the web."*

The right demand. A citation builder validated against its own inputs proves only that it is
self-consistent. So: seventeen papers across six unrelated fields — potato late blight, CRISPR crop
editing, protein-structure prediction, soil carbon, malaria vector control, Andean glacier retreat
— built with `minime_local.citations`, then every DOI resolved at **Crossref** and every field
compared with what the registry holds.

| | |
|---|---|
| DOI resolves at Crossref | **17 / 17** |
| title agrees | **17 / 17** |
| year agrees | **17 / 17** |
| volume — agrees / omitted / **contradicts** | 9 / 8 / **0** |
| pages — agrees / omitted / **contradicts** | 8 / 9 / **0** |

**Zero contradictions.** Where Semantic Scholar carries a field, it matches the registry; where it
does not, the reference renders without it. That is the module's whole discipline holding under
measurement rather than by assertion: eight references are missing a volume and none of them is
wrong about one.

Set against the same pipeline three days ago — six references, three DOIs resolving to different
papers and three to nothing at all — the difference is not that the model got better. It is that
the model is no longer the one producing the field.

### What the check found that the unit tests could not

Two rows first looked like disagreements and were not: Semantic Scholar indents `pages` across
newlines, so the raw value is `"\n          1-8\n        "`. `_clean` already collapses it and the
rendered citation was correct — but only the *live* comparison surfaced that the raw data has that
shape at all. Now pinned as a case, with the real value in it.

*A formatter tested against data its author invented is tested against their idea of the data.*

### The result that matters is the one on the left

`17/17` DOIs resolving is the headline, and it is worth being clear about *why* it is not
impressive engineering: the DOI is copied out of the record it came with. It is the number that
was wrong before because the number was previously being remembered instead of copied.

## 127. One repo moved on `git pull`, and it was the wrong one (2026-08-07)

> *"wait what, why I need to put something in wsl and not by doing git pull; cargo run?"*

A fair question with an embarrassing answer: because the backend never updated at all.

`setup-wsl.sh` clones Mini-Me once and says so in its own header — *"never overwrites an existing
checkout, and never touches a checkout it did not create."* That is the right rule for
provisioning and it was doing the whole job: there was no pin, no update path, and nothing that
moved the checkout afterwards. The Python the agent actually runs stayed at whatever commit it was
cloned at, forever.

It went unnoticed because everything shipped so far lived in the *desktop* repository — including
`overlay/minime_local/*.py`, which travels with the Rust app and reaches into the backend at
import time. `git pull; cargo run` was genuinely enough. The moment a fix landed in Mini-Me itself
(§126), the gap appeared, and the instruction that fell out of it was "type this into WSL" — which
is precisely what a researcher who cannot code must never be asked to do.

### The pin

`Settings::backend_ref` names the Mini-Me version this build expects, defaulting to a constant in
this repository. So the pin travels with `git pull`, and `BackendSupervisor::sync_to_pin` brings
the checkout to it on the next launch. One command again.

A setting as well as a constant, because a developer testing an unmerged backend branch should not
have to rebuild the Rust app, and someone whose network cannot reach GitHub should be able to pin
themselves in place.

### The guards are the part worth reviewing

Getting the happy path wrong costs a stale backend. Getting these wrong destroys a colleague's
uncommitted work, which is the failure §231's ownership flag exists to prevent.

* **Not ours, not touched.** `owned` is false for any checkout the app merely found or was pointed
  at — the reference checkout on this machine has ten local branches, several live in worktrees.
* **Dirty, not touched.** Refused rather than stashed or forced. A dirty tree means somebody is
  editing the backend, and this is not the code that decides what happens to that.
* **Never blocks the launch.** Offline, no `git`, a ref that does not exist — each logs and
  returns. A backend one version behind still runs; a backend that would not start because the
  network was down would be a worse app.
* **Already at the pinned commit costs nothing.** A local `rev-parse` before any network call.

Tested against a real temporary repository rather than a mock, because the thing being verified is
what `git` does, and a mock of git would only confirm what I believe git does.

### Logged on success

`sync_to_pin` says when it moved the backend and to what. Three overlay patches hid this week
behind installers that spoke only on failure (§123, §125), and a version sync is exactly the kind
of thing where "already right", "moved" and "silently skipped" must not produce identical evidence.

*Two repositories is a real cost, and this is the first instalment of it. The answer is not to
avoid the split — it is that the seam has to be as automatic as the thing it replaced.*

## 128. `ast.parse` is not "it runs" (2026-08-07)

Every turn failed with *"An internal error occurred"*. The backend log:

```
NameError: name 'name' is not defined
```

Mine, shipped an hour earlier, in the diagnostic added to tell §127's two causes apart:

```python
async def _watched(*args, _inner=coroutine, _tool=name, **kwargs):
```

`name` is defined in **`_make_mcp_tools_resilient`** — upstream's loop, whose body I had read while
working out where to hook — and not in `_recording`, which is ours. Python evaluates a default
argument when the `def` executes, so it raised the moment the tool list was wrapped, on every turn,
before any work began.

### How it went out

I checked it with:

```
python3 -c "import ast; ast.parse(open('overlay/minime_local/sources.py').read())"
```

which proves a file is syntactically valid and says nothing about whether a line of it runs. Then
`cargo test`, which exercises the Rust and never touches the overlay. Both passed. Neither could
have failed.

*A check that cannot fail for the reason you are worried about is not a check.*

### The pattern, now four deep

This is the same borrowed-from-a-file-I-do-not-own mistake as §113 (restated a signature), §125
(guessed a private function name, twice) — and now a bare identifier lifted out of somebody else's
scope. Each time the fix was correct in intent and wrong about a detail of code the overlay reaches
into but does not own.

The overlay's whole premise is patching internals by name from outside. That premise has now cost
four defects in three days, and it is the strongest argument yet for §126's direction: this belongs
*in* Mini-Me, as ordinary code, where a name that does not exist is a name a reader can see.

### What changed besides the fix

`the_overlays_tool_wrapper_runs` drives the real arrangement from Rust — upstream's wrapper around
ours, the tool call in a child task — and asserts the store afterwards. Verified load-bearing by
reintroducing the bug and watching it fail with the same `NameError`.

That is the third overlay path now covered by an executing test rather than an imagined one, after
§123's task boundary and §125's hook name. All three were written *after* the failure they would
have caught, which is the honest description of this week.

## 129. The pin pinned itself, and the guard guarded against us (2026-08-08)

§127's version sync shipped, and the backend still did not move. The line added an hour later said
why, in one go:

```
WARN the backend checkout has uncommitted changes — leaving it at its current
     version rather than overwriting them   want=desktop_to_web
```

Two defects in eleven words.

### `want=desktop_to_web`, after that branch had merged

`backend_ref` was a **saved setting** with a constant default. The constant had already moved to
`main`; `settings.toml` had `desktop_to_web` written into it by an unrelated save — a panel toggle
is enough — and a persisted value beats a default forever.

Which destroys the only property the pin has. The whole point is that the version travels with
`git pull`; a setting that overrides it means the first researcher to run any build is frozen at
that build's pin for good. **The pin pinned itself.**

Now a constant with `MINIME_BACKEND_REF` as the override. An environment variable is the right
shape for "test an unmerged branch": deliberate, scoped to one session, impossible to leave behind
by accident. The setting was over-engineering that broke the feature.

### "Uncommitted changes" that were ours

The app writes into that checkout **by design** — the generated `.mini-me-desktop.langgraph.json`
(§30) and the copied `.desktop-overlay/`. So `git status --porcelain` is never empty there, and the
guard fired on every launch, reporting somebody's work in progress where there was none.

`--untracked-files=no`. What the guard is for is a *tracked* file somebody edited, which is the
only thing a checkout could actually lose.

### The test that passed with the bug in it

Worse than either. The first version wrote the untracked files and then synced to `Some(&before)` —
the commit already checked out — which returns at the *"already at the expected commit"* line,
before the dirty check is reached. Both the fixed and the broken code left `HEAD` where it was, so
the assertion held either way.

Caught only by deleting the fix and re-running, which is now the habit worth keeping: **a test
written for a bug should be watched failing on that bug before it is trusted.** Rebuilt around a
bare repository standing in for `origin` and a branch the checkout can actually move to, so a
blocked sync and a working one give different answers. Verified failing without
`--untracked-files=no`.

*Three of this week's tests passed while the defect they were written for was still present
(§125, §128, and this one). Each was a test of what I meant rather than of what the code does.*

## 130. The sync ran after the return that skipped it (2026-08-08)

Third launch reporting nothing wrong, backend still on a commit from two merges ago.

`sync_to_pin` was called inside `ensure_running` — **below** this:

```rust
if client.is_healthy().await {
    return Ok(Started::Attached);
}
```

`langgraph dev` survives the app closing. So "a backend is already running" is not an edge case,
it is what happens every time somebody restarts the app without killing the sidecar — which is
every time. The sync sat on the branch that only runs when there is *no* backend, which is the one
launch where the version was already going to be read fresh anyway.

Worse, §129 *found* this and logged it instead of fixing it: `"a backend was already running —
attaching to it, so the version pin is not applied"`. An accurate line about a thing that should
not have been true. Naming a defect is not the same as repairing it, and the line went out in the
same commit that could have moved three statements.

Now the sync runs first, unconditionally. And when it moves the checkout while a server is already
up, the app says what that means and what to do:

> the backend files were updated, but a server was already running and is still on the old ones —
> close this app, then run: `wsl bash -lc "pkill -f 'langgraph dev'"`

`sync_to_pin` returns whether it moved anything, because the warning depends on it and a bool that
nothing asserts is a bool that drifts.

### Four bugs in one delivery mechanism

§127 built it; §129 found the pin overriding itself from `settings.toml` and the dirty guard
tripping on the app's own untracked files; this one is the ordering. Every one of them produced the
same visible outcome — the researcher pulls, the app starts, nothing has changed — and each had a
different cause.

*A mechanism whose failure mode is silence needs its diagnostics written before its logic, not
after each failure.* Every fix in this chain arrived one launch late because the line that would
have named the cause did not exist yet.

## 131. A version check that waited for a password (2026-08-08)

*"dont know why the backend takes to long to start."*

§130 moved `sync_to_pin` above the health check, which was right, and made it the first thing every
launch does — which turned two of its properties into defects.

`run_git` shelled out with no timeout and no prompt suppression. Mini-Me is **private**, and once a
credential helper is configured, `git fetch` against it does not fail — it *waits*, for a sign-in
dialog nobody is watching for, before the backend is spawned. The app looks hung, and the thing it
is hung on is a version check.

Now: `GIT_TERMINAL_PROMPT=0`, an askpass that answers nothing, and `timeout 20`. "Ask the user"
becomes "fail immediately", and the rest — DNS, a stalled handshake, a moved repository — is
bounded. A version check is worth a few seconds and worth nothing at all if it costs a window that
will not open.

### Where the rest of the startup goes

Measured from a real launch, spawn to healthy is about **17 seconds**:

| | |
|---|---|
| `asta auth print-token --raw --refresh` | **~10s** |
| `langgraph dev` importing the graph | ~7s |

The token is minted **fresh on every launch** (`backend.rs:1423`) — `--refresh` unconditionally,
with no check of whether the stored one is still valid. That is the single largest cost in getting
the window usable, and it is a network round trip through the CLI for a credential that is
typically still good. Worth a validity check before a refresh; not tonight.

The MCP tool lists are a separate ~7s, paid on the **first turn** rather than at startup — 8 Asta,
23 Dataverse, 9 AGROVOC, each fetched over HTTP when the agent is assembled.

*The startup path is the one place where every network call is a call the researcher waits on with
nothing on screen. It deserves an inventory, and it has never had one.*

## 132. The diagnostic that reads the same when it works (2026-08-08)

> `0 of 7 sources carry the corpus id Asta returned`

That line is the whole of the evidence four days of DOI work has been steering by, and tonight —
with `find_papers` finally on the machine and the skills path fixed — it printed again, unchanged.
The reading each previous time was *the subagent called no search tool and wrote its citations from
memory*. That reading is no longer sound, and the line is the reason.

### What it actually measures

`_seen` is filled by `install_mcp`, which hooks `_make_mcp_tools_resilient` — the function that
wraps the **MCP bundle**. `find_papers` is ours: a plain `@tool` in `backend/paper_tools.py`,
handed to the subagent alongside the bundle and never through it. It has never passed through that
wrapper, so nothing it returns is recorded.

So `_papers()` is empty whenever the CLI path is used — *including when it works perfectly* — and
`link_for` returns nothing for every source, and the count is zero out of seven. The line reads
identically in the two cases it exists to tell apart:

| what happened | what the line says |
|---|---|
| no search ran; citations came from memory | `0 of 7` |
| `find_papers` ran; every link built from the record | `0 of 7` |

§81's rule, for the fifth time this week, and this time it cost the diagnosis rather than a
launch: the message was written when there was one way to find a paper, and a second way was added
without revisiting what the message claims.

### The other half: a level nobody checked

`find_papers` does log its result — `logger.info("find_papers(%r) -> %d paper(s)")`. Every line
this overlay has ever been *seen* to produce in the backend log arrived at WARNING, and the spawn
sets no log level. So the absence of that line was read as "the tool did not run" when it may only
mean "INFO does not reach this file". A diagnostic on an unconfirmed channel is not a diagnostic.

### The fix

`install_papers` wraps `find_papers` the way `install_mcp` wraps the bundle, and says so at the
level that demonstrably arrives:

> `minime_local: find_papers returned 10 paper(s), 10 newly recorded (10 known so far)`

`_seen` now stores a **finished URL** rather than a corpus id, because there are two shapes of
answer and only one of them carries an id. `find_papers` arrives with a link already resolved
against the publisher's record by `backend/citations.py`, which prefers the DOI when there is one —
strictly better than anything reconstructible here. Storing the id would have meant discarding it
and rebuilding something worse.

And the artifact line no longer reports a count when there is nothing to count:

> `no search recorded — the 7 source(s) keep the links the subagent supplied, which are their own
> unless find_papers logged above`

The tool object is **mutated**, not rebound: `backend/agent.py` does `from backend.paper_tools
import find_papers`, so by patch time the agent already holds the object and replacing the module
attribute would patch a name nothing reads — the §125 failure exactly, which took two attempts
there and should not take a third here.

`the_overlay_records_the_cli_search_as_well_as_the_mcp_one` drives it from Rust, through a child
task, and asserts the DOI link survives rather than being rebuilt as a corpus id.

*The question that mattered tonight — did the subagent call a tool — was answerable all along by a
line that was never written, and unanswerable from the one that was.*

## 133. The exit that was always cheaper than the work (2026-08-08)

> `minime_local: recording the link of every paper find_papers returns`
> `minime_local: no search recorded — the 8 source(s) keep the links the subagent supplied`

The wrapper installed, the tool was present, and the subagent called **nothing**. Eight references
composed from memory. §132 made the question answerable; this is the answer.

### It was never the prompt

Four days went into the prompt — *"Use available tools"*, then a whole appended block of identifier
rules on top of it, and the behaviour did not move once. It could not have.

`academic_researcher` carries `response_format=AcademicResearchResults`. Anthropic models report
`structured_output: False` in their profile, so LangChain resolves that to a `ToolStrategy`: **the
schema is bound as a tool.** And then, in `langchain/agents/factory.py:1273`:

```python
# Force tool use if we have structured output tools
tool_choice = "any" if structured_output_tools else request.tool_choice
```

The first model call is *compelled* to call a tool. Among its options is one that answers the whole
question in a single step, from memory, and ends the episode. Every other option is work. The model
was doing the rational thing with the choices it had, every single time, and a sentence in a system
prompt asking for diligence was never going to outweigh the shape of the loop it runs in.

### The line that also decided the fix

That same statement discards `request.tool_choice` whenever a structured output tool is bound. So
the obvious repair — middleware that sets `tool_choice="find_papers"` — writes a value nothing
reads. It would have installed cleanly, logged nothing wrong, and changed no behaviour: the exact
shape of §125, §129 and §131, and it would have taken another launch to notice.

Found by reading the binding path before writing the middleware rather than after it failed. That
is the only reason this is one section and not three.

### The fix

Withhold the exit until the work is done:

```python
request.override(response_format=None, tool_choice=SEARCH_TOOL)
```

Both halves are load-bearing. Dropping the response format un-binds the structured output tool,
which is what lets `tool_choice` reach the model at all; naming the tool makes the forced call a
*search* rather than whichever of `ls`, `execute` or `write_todos` it picks when told only that it
must call something.

The gate opens on a search result **existing**, not on it being useful — an empty result, a
timeout, a missing sandbox each still leave a `ToolMessage` behind. A failed search costs a
citation and never a turn.

Upstream as [Mini-Me #40](https://github.com/CENTRO-INTERNACIONAL-DE-LA-PAPA/Mini-Me/pull/40), in
the repository rather than the overlay: the constraint that put four of these patches in
`overlay/` was *"the checkout is reference material"*, and that ended with *"I myself mantain it"*.

*A tool the model may call is a suggestion. The whole DOI investigation — corpus ids, Crossref
verification, a citation builder, a CLI search — was built on the assumption that the subagent was
searching badly. It was not searching at all, and nothing we added on that assumption could have
found that out.*

## 134. A fix that was merged, pulled, and never delivered (2026-08-08)

§133's fix was merged at 23:22:57. The run at 23:24 behaved exactly as before, and the honest
reading of that was *"the fix does not work"*. It had not run. The backend was still executing
`77c8b94` — **three pull requests behind**.

Every layer of the delivery chain reported success or said nothing:

- `sync_to_pin` fetched from Mini-Me's own remote. It is a **private** repository and WSL holds
  no credentials for it, so the fetch failed — quietly, and into the *app's* log, not the backend
  log anyone reads.
- The checkout also carried four hand-delivered files from an earlier attempt, so the dirty-tree
  guard refused to move it. Correctly. It cannot tell an abandoned manual patch from a
  colleague's work in progress.
- And the backend log carried **no version at all**, so every line in it was read against an
  assumption about which code produced it.

Three independent safeguards, each behaving as designed, combining into a backend frozen a month
back with nothing on screen to say so.

### The stamp

The overlay now logs the checkout's commit as its first line — read from `.git` directly, no
subprocess, because this runs during start-up and a stalled `git` would delay the window:

> `minime_local: backend checkout aab5790 (main)`

It reads a linked worktree correctly too, via `commondir`, which it did not on the first attempt.
A diagnostic that cannot read its own repository is worse than none: it earns the shrug it exists
to prevent.

## 135. One repository (2026-08-08)

> *"from now I want a mono repo in mini me desktop. copy everything we need I dont want to depende
> on a secod repo anymmore"*

The backend source is now `mini-me/` in this repository, tracked, at `aab5790` — all 184 files,
including the web frontend, so nothing is left behind that would keep the second repository alive.

### What §5 was protecting, and why it stopped paying

The locked decision was *"bundled, never forked"*, and its reasoning was sound: a vendored copy in
git is a fork with extra steps, and a fork drifts. That held while the checkout was **reference
material somebody else maintained**. It stopped holding at *"I myself mantain it"*, and what
replaced it was not safety but four delivery bugs in three days, ending in §134 — a merged fix that
never reached the machine because the update path needed credentials the machine did not have.

The monorepo removes that path entirely. `bundled_backend_dir()` finds `mini-me/` first, and
provisioning already knew how to copy from a bundled source rather than clone
(`scripts/setup-wsl.sh`, `MINIME_BUNDLED_SOURCE`). **A `git pull` on this repository is now the
backend update** — a file copy that needs no network and no token, instead of a `git fetch` against
a private remote that WSL has never once succeeded at.

### What is deliberately not done yet

`sync_to_pin` still updates an existing checkout with `git fetch origin`. Provisioning a *new* one
now uses the in-repo source, but a machine that already has `~/Mini-Me` keeps the old path, and the
old path is the broken one. Replacing it means deciding what a WSL checkout even is once the source
is here — a git clone with its own history, or a plain directory mirrored from `mini-me/` with a
version stamp written beside it. The second is simpler and matches the monorepo, and it wants a
clear head rather than the end of this session.

*Written down rather than done, because the failure being fixed is a delivery mechanism whose
failure mode is silence — and shipping half of one at midnight is how §127, §129, §130 and §131
each happened.*

## 136. Keeping the vendored copy honest (2026-08-08)

`mini-me/` is re-vendored from Mini-Me `main` after each merge, whole, never patched in place:

```sh
rm -rf mini-me && mkdir mini-me && git -C ../Mini-Me archive <sha> | tar -x -C mini-me
```

**The rule is: `mini-me/` equals an upstream commit, exactly.** Editing it directly is how a
vendored copy becomes a fork, which is precisely what §5's "bundled, never forked" was protecting
against and the one part of that decision worth keeping. A change to the backend goes upstream
first and arrives here by re-vendoring, so `mini-me/` is always a commit somebody can name.

Now at `8017be5` — [#42](https://github.com/CENTRO-INTERNACIONAL-DE-LA-PAPA/Mini-Me/pull/42) and
[#43](https://github.com/CENTRO-INTERNACIONAL-DE-LA-PAPA/Mini-Me/pull/43), which between them
stopped the academic researcher discarding most of what it found.

## 137. Ten of eleven, and the eight nobody counted (2026-08-09)

The first run with #40 and #42 both live:

> `find_papers('late blight resistance in Andean potato landraces …') -> 20 paper(s)`
> `minime_local: 10 of 11 sources relinked to a paper the search returned (19 recorded)`

The gate holds and the links are real. Two things that line does not say:

**Which source was the eleventh.** "10 of 11" has two causes needing opposite fixes — a paper the
model added from memory *after* searching, or a real one whose citation drifted far enough that
`link_for`'s ambiguity guard refused to choose between two near-identical titles. With nineteen
papers on one topic, near-ties are the expected case, not the exotic one. The line now prints the
citation, because §81 keeps being the answer.

**That eight papers never reached the answer at all.** Nineteen recorded, eleven reported. The
count of what was relinked says how much of the answer is anchored to a record; it says nothing
about how much of the search was dropped before the researcher could see it. A run that retrieves
nineteen papers and shows eleven scores ten-of-eleven and reads as healthy.

That second number is the researcher's actual request — *"is up to the scietinst to selct and drop
the ones they want"* — and #42 asked the model for it in a prompt. It filtered anyway. Which is
tonight's lesson for the second time: **a prompt is a request, and the model is free to decline.**
The search only became reliable when the alternative was removed from the loop rather than argued
against, and reporting will likely need the same treatment — the papers `find_papers` returned are
already recorded here, so the artifact can carry them whether or not the model chose to mention
them.

*Measured first, on purpose. The last structural fix was built on four days of assuming the wrong
cause; this one starts with a number.*

## 138. What the DOI thread was actually about (2026-08-09)

> *"the links are great!"*

Five days, and the citations now come from the publisher's record, the links open the paper they
name, and nothing the search finds is thrown away. Verified on a live run: `6 of 6 sources
relinked to a paper the search returned (6 recorded, 0 never reported)`, and by opening them.

### The cause, and how far away it was from where we looked

The subagent was never searching badly. **It was not searching.** Its structured response was bound
as a tool, LangChain forces `tool_choice="any"` whenever one is present, and among its options sat
one that answered the whole question from memory in a single step and ended the turn. It took the
cheapest legal move, every time.

Everything built before that — the corpus id capture, the Crossref verification, the citation
builder, the CLI search, the automatic link repair — was correct, is still in use, and **none of it
could have found this**, because all of it assumed a search was happening. Four days of work
downstream of an assumption nobody had checked. The check, when it finally came, was one log line
saying whether a tool had been called.

### The rule this subsystem kept teaching

Three times in one evening, in three different places:

| asked the model | outcome |
|---|---|
| *"Use available tools"* | it did not (§133) |
| *"report every paper, rank rather than filter"* | reported 9 of 24 (§137) |
| *"use the citation exactly as given"* | rewrote them (§119) |

And three times, removing the alternative from the loop held: withhold the structured response
until a search returns; append the missing papers where the list leaves the backend; build the
reference in code so there is no field left to invent.

**A prompt is a request the model is free to decline. A structure is not.** Prompts are still the
right tool for judgement — which papers bear on the question, how to rank them — but never for an
invariant. If it must be true, it cannot be asked for.

### And the corollary, which cost more than the bug

Every one of those three was diagnosed from a log line that could not distinguish success from
failure. `0 of 7 sources carry the corpus id` printed identically whether the search had worked
perfectly or never run (§132). `10 of 11 relinked` read as healthy on a run that dropped sixteen
papers (§137). Both were written when the system had one path, and neither was revisited when it
grew a second.

*The diagnostics were wrong for longer than the code was, and they are why the code stayed wrong.*

## 139. One repository, finished (2026-08-09)

> *"I want minime desktop as a mono repo so we dont need to pull and copy the backend"*

§135 put the source in `mini-me/` and left the update path alone, which meant a fresh machine
provisioned from this repository and every existing one still fetched from Mini-Me's private
remote. That half is now gone.

### What was removed

`sync_to_pin`, `run_git`, `backend_ref`, `MINIME_BACKEND_REF` and the pin tests — about 220 lines.
Deleted rather than disabled. Machinery that cannot run is not neutral: three of this week's four
delivery bugs were safeguards behaving exactly as designed, in a combination nobody had in mind,
and a dead `git fetch` sitting behind a live file copy is the next one waiting.

### What replaced it

`sync_source_command` — the same shape as `sync_overlay_command`, which has worked since §25:

```sh
for d in backend skills; do
  rm -rf DIR/.$d.new && cp -r SRC/$d DIR/.$d.new && rm -rf DIR/$d && mv DIR/.$d.new DIR/$d
done
```

Staged beside the target and swapped in only once the copy has fully succeeded, so an unreachable
source — a Windows drive that is not mounted, the case the in-distro copies exist to survive —
leaves the working checkout alone instead of deleting it. `uv sync` runs only when `uv.lock`
actually moved, compared against a stamp; without it a dependency added upstream would surface as
an ImportError at boot, naming the wrong problem.

**Stdout is discarded and stderr is not.** A mirror that failed silently would be §134 in a new
coat.

Only for a checkout the app owns — someone who pointed us at their own clone keeps it. That was
the one part of the pin worth keeping.

### What this buys

`git pull` on this repository *is* the backend update. No network, no token, no second remote, no
copying by hand. The thing that cost four test cycles on 2026-08-08 cannot recur, because the
mechanism it depended on no longer exists.

**One case remains, and it is honest about itself:** a `langgraph dev` left running from a previous
session has already imported its code, and nothing at launch can change that. The app now says so
on attach, with the command to fix it, rather than reporting success.

## 140. Where else a prompt is holding an invariant (2026-08-09)

> *"Do you think each async subagent needs specific customed middlewares?"*

Not each. Only where a subagent has an invariant a prompt cannot enforce — and there is a way to
find those without guessing. **Grep the prompts for shouting.** Every `NEVER`, `ALWAYS` and
`Mandatory` in `backend/subagents.py` marks a place where somebody already suspected the model
would decline, and wrote in capitals instead of in code:

| subagent | the rule, in its own words | checkable from the run? |
|---|---|---|
| `dataverse_explorer` | *"Mandatory fixed filename rule: ALWAYS call `SearchCIPDataverse` with `output_filename="dataverse_search.json"`"* | yes — the tool call's arguments |
| `hypothesis_generator` | *"Never fabricate theories or citations. ALWAYS return a structured HypothesisOutput"* | yes — did `generate_theories` return? |
| `pdf_librarian` | *"NEVER claim you extracted... Never fabricate documents or matches"* | yes — did the extraction tool return? |
| `data_voyager`, `diagnostic_analytics` | *"NEVER invent findings, numbers, or charts"* | yes — did an analysis tool run? |
| `report_writer` | *"the `markdown` field MUST contain the full report content"* | yes — a shape assertion on the output |
| `research_planner` | *"You NEVER run anything"* | **already structural** — `tools: []`, `skills: []` |

`research_planner` is the contrast worth keeping in view: its rule is not a rule, it is a fact
about what it was given. Nothing to enforce, so nothing to write.

### Every one of these shares the §133 exit

Each carries a `response_format`, which LangChain binds as a tool and then forces
`tool_choice="any"` around. So on the first model call each of them is compelled to call
something, and each has one option that answers the whole question from memory in a single step
and ends the turn. `academic_researcher` took it every time for four days. There is no reason to
believe the others behave differently — only that nobody has looked, because their output is
harder to check than a DOI.

**`dataverse_explorer` first.** It is the same failure with a shorter fuse: a `DataVerseSearchResults`
composed from memory carries invented `persistent_id`s, and a persistent id is a thing a researcher
will paste into a citation without thinking, exactly as they clicked the DOIs. Its mandatory
filename rule is purely mechanical besides — a wrapper should set that argument, not ask for it.

*The rule this project keeps arriving at: if it must be true, it cannot be asked for. What is worth
adding is that the places to look are already marked, in capital letters, by whoever wrote the
prompt.*

## 141. The report the bibliography took down with it (2026-08-09)

The literature path started working, so a researcher did the next obvious thing and pressed
**download as PDF**:

```
the backend could not render the report (502 Bad Gateway):
{"error":"PDF render failed: 'str' object has no attribute 'get'"}
```

`_build_typst_wrapper` reads each entry of `sources` as a mapping:

```python
citation_raw = (source.get("citation") or "").strip()
```

And this app sent a list of bare citation strings:

```rust
self.sources.iter().map(|source| source.citation.clone()).collect(),
```

under a comment stating, as fact, *"the backend's Typst template takes a list of citation
strings."* It does not, and never did.

### The comment was the whole bug

Nobody wrote that line carelessly. §110 turned this route on — *"the rendering already existed and
had never been called"* — and reading a Typst template that emits `- {citation}` per line, a list
of citation strings is the obvious thing it wants. The `.get()` two lines above it decides
otherwise, and the code was never run against a report that had a source, so the belief was never
contradicted.

This is the project's recurring shape at its purest: **a distinction that lived in a comment and
not in the data.** Two clients read the same undocumented field two reasonable ways, and the field
had no opinion. `render_report` validates that `sources` *is a list* and stops — the container's
type is a contract, its contents are not.

### Why it stayed hidden for a year

```python
for source in sources or []:
```

With no sources, the loop that dies never runs. Every report without citations rendered perfectly,
which is nearly all of them until §133 made the searching reliable. **The feature that fixed the
citations is what made this reachable** — the second time in this thread that getting something
working exposed the thing behind it.

### Fixed on both sides, deliberately

**The app now sends the objects** (`protocol.rs::render_request_body`), which is not just the
narrow fix. `Source` has carried `link` since §91 — the field whose own docstring explains that a
model-written DOI is *"usually right, and wrong without warning"* while the real one sits one key
away. The mapping to `Vec<String>` threw it away at the last step, so no rendered bibliography had
ever had a resolvable link in it. **The §91/§115 shape again, in the one place it had survived: a
value the program already had and never read.**

Building the body is now a pure function beside `decode_sources` rather than JSON assembled inline
inside an HTTP call, because the wire shape was untestable where it was — the only way to see it
was to make the request. Same reasoning as upstream's `_build_search_command`, arrived at from the
opposite direction.

**The route now accepts either shape** (Mini-Me PR #44). Four lines, and the next client to guess
differently gets a PDF instead of a stack trace. An entry it truly cannot read is dropped *with a
warning*, because a bibliography quietly one entry short is `paper_tools.unreported`'s failure
arriving through a different door.

Verified with the exact payload that produced the 502: `PDF bytes: 35382 b'%PDF-'`. Nine Python
tests, one Rust test that asserts every entry `is_object()` — 210 and 228 green.

## 142. The second of nine (2026-08-09)

§140 listed the subagents holding an invariant in a prompt and named `dataverse_explorer` first.
This is that one, as Mini-Me PR #45.

The exit is identical to §133's: `response_format=DataVerseSearchResults` is bound as a tool,
`tool_choice="any"` is forced, and one of the options answers the whole question from memory in a
single step. What comes out when it does is a list of `DataVerseFindings`, each carrying a required
`persistent_id` — *"Dataset DOI or persistent identifier"*. **A researcher pastes that into a
citation without checking it**, exactly as they clicked the DOIs. And unlike an invented reference,
a wrong persistent id cannot be caught by recognising the title.

### Two steps, because one proves nothing

`SearchCIPDataverse` writes its results to a **file**. `read_search_results` is what puts the
metadata in front of the model. So a gate that opened as soon as a search returned would let the
subagent search, satisfy the gate, and still compose every field from memory — having demonstrated
only that it can call a tool. Both are forced, in order. `list_dataset_files` is not: it is for
shortlisted datasets only and nothing in the schema depends on it.

### The rule came out of the prompt

    Mandatory fixed filename rule: ALWAYS call `SearchCIPDataverse` with
    `output_filename="dataverse_search.json"` and ALWAYS call `read_search_results` with
    `filename="dataverse_search.json"`.

Two tools that must agree on one string, spelling the argument differently on the way out and the
way back. That is mechanical, so `FixedSearchFilename` sets it in the call. **The paragraph is
deleted rather than kept beside the middleware** — a rule that is enforced *and* still requested
teaches the next reader that the prompt is where such things live, which is the belief this whole
sequence has been dismantling.

### Why it is a base class now

The mechanism moved to `middleware/tool_gate.py`. Seven subagents still carry the same exit, and
writing it a third time by hand is how the ninth ends up subtly different from the first.
`SearchBeforeCiting` became a subclass **with its seven tests unchanged and passing**, which is the
only proof worth having that a refactor of the one thing that finally worked changed nothing. Its
log line is byte-identical too, so anything grepping `has not searched yet` still matches.

Two of the sixteen new tests run against LangChain's real `ModelRequest` and `ToolCallRequest`
rather than a double, because this entire family of bugs is *a value written where nothing reads
it* and a hand-written stub cannot catch that by construction.

**A comment in `subagents.py` first claimed `FixedSearchFilename` had to be outermost.** Checked:
the two override disjoint hooks — `wrap_model_call` and `wrap_tool_call` — so neither composes
around the other and the order is free. One day after §141, which was a comment asserting a
contract the code did not have.

### Verified, and the filename result is the finding

Both gates fired on the first live run, and all four persistent ids resolved to real CIP datasets
— *Stability of resistance and yield of 15 advanced clones* (2016), two *Participatory Varietal
Selection* datasets from La Libertad (2017), and *Phenotypic Stability for Tuber Yield and Late
Blight Resistance in B3C3* (2018). On topic, and real.

The filename log is the part worth keeping:

```
SearchCIPDataverse(output_filename='cip_late_blight_peru.json') -> 'dataverse_search.json'
SearchCIPDataverse(output_filename='q1.json')  -> 'dataverse_search.json'
SearchCIPDataverse(output_filename='pvs.json') -> 'dataverse_search.json'
read_search_results(filename=None)             -> 'dataverse_search.json'
```

**Nine searches, nine different names, and no filename at all on every read.** The prompt said
*"ALWAYS… Do not invent or vary this name"*, and compliance was zero out of twelve. Which means
`dataverse_explorer` was not merely at risk of inventing datasets — **it was broken**: every search
wrote to a file no read would look for, and the subagent narrated around the failure convincingly
enough that nobody had noticed. A capital-letter rule was the only thing holding a two-tool
handshake together, and it held it none of the time.

*One id looked like a five-character truncation and was called a transcription loss here before the
researcher pointed out the missing character was in their paste. The model got all four right. It
is worth recording that the first instinct on seeing a bad identifier is now to blame the model,
and that instinct was wrong.*

### The copy that reported `ok` and had not

The first real-machine run of §139's monorepo install failed two steps *after* the mistake:

```
==> Copying Mini-Me from /mnt/c/.../mini-me-desktop/mini-me
    ok  copied to /home/piero_linux/.local/share/mini-me-desktop/backend
==> Installing Python packages
error: No `pyproject.toml` found in current directory or any parent directory
```

`cp -r SRC DEST` means two different things. When `DEST` does not exist it *becomes* a copy of
`SRC`; when it does, it gains a `DEST/<basename SRC>`. `setup-wsl.sh` removes `$DIR` only when it
is *empty*, so a directory left behind non-empty by anything at all turned the copy into
`$DIR/mini-me/pyproject.toml` — and reported `ok`, because the copy had in fact succeeded.

Trailing `/.` copies the contents and means one thing only. The same distinction bit
`sync_source_command` while §139 was being written, caught there by a test; here it reached a
researcher, because the setup script's only test is running it.

**And the check that was missing is the cheap one**: the step now asserts `pyproject.toml` is where
it will be needed, instead of leaving a wrong layout to be discovered by the first command that
required a file. A diagnostic that reads the same on success and failure is §132's shape, and this
is the fourth time it has appeared in this document.

## 143. Sixteen outputs that were one directory too deep (2026-08-09)

§117's real case is fixed at the source: `workspace::outputs` now descends into an agent's named
folders, so `eda_outputs/yield.png` and `eda_outputs/tables/summary.csv` reach both the Outputs
panel and the transcript diff. The displayed name is the path relative to the conversation rather
than only the basename. That preserves the useful folder name and keeps two `summary.csv` files in
different analyses distinguishable.

The walk is deliberately not general-purpose file indexing. It stops after four subdirectory
levels, 2,048 directory entries or 512 files; skips dotted entries and `__pycache__` at every
level; and never follows symlinks. When any bound bites, the panel says it is showing a bounded
view and points to the folder for the rest. A silent cap would have reproduced the same defect at
file 513.

Three sentence-named tests pin the behaviour: nested output folders remain visible, hidden caches
remain hidden below the top level, and a deeper tree reports truncation instead of pretending the
scan was exhaustive.

## 144. CRLF was not uncommitted work (2026-08-09)

`setup-wsl.sh` copies an existing Windows checkout into the distro, including its Git index and
working files. It does not copy Git for Windows' *global* `core.autocrlf=true`. WSL Git therefore
saw the CRLF bytes the Windows checkout deliberately contained without the policy that normalised
them for comparison and reported essentially every tracked file as modified.

Provisioning now sets `core.autocrlf=input` in the copied checkout, on new installs and re-runs.
That normalises CRLF when Git reads it, while future checkouts made inside WSL stay LF. Git caches
the previous clean filter in its index, so the script runs `git add --renormalize -- .` once — but
only when the ordinary diff is non-empty and `--ignore-cr-at-eol` proves every unstaged difference
is the line ending. A genuine edit leaves the tree untouched. It does not run `reset --hard` or
rewrite working files: `find_source` can copy a developer's checkout with genuine edits, and
fixing line-ending interpretation does not authorize destroying those edits. One regression test
pins the shipped script and absence of a hard reset; another
creates an LF-indexed repository, replaces its working file with CRLF bytes, proves it is dirty
under `core.autocrlf=false`, and proves the same bytes are clean under the installed `input`
policy.

The shell parser could not be run in this Windows sandbox because creating a WSL instance returns
`E_ACCESSDENIED`; the Rust test that embeds the shipped script passes. The remaining proof belongs
on the target machine: after Setup finishes, `git status --short` inside the provisioned checkout
must print nothing.

## 145. The token already had six days left (2026-08-09)

§131 measured about ten of seventeen startup seconds in
`asta auth print-token --raw --refresh`. The command forced a network refresh on every backend
spawn even though a real Asta JWT says `exp - iat = 604800` — seven days.

Startup now takes the cheapest valid answer in order:

1. the `ASTA_TOKEN` already read from the OS keychain, with no subprocess;
2. the CLI's cached `print-token --raw`, without `--refresh`;
3. a fresh `print-token --raw --refresh`, only when the first two are absent, malformed or near
   expiry.

"Near" means five minutes. That margin is negligible against seven days and avoids beginning a
turn with a credential that can expire while LangGraph imports the graph or the agent assembles
its MCP tools. The app base64url-decodes only the JWT payload and reads numeric `exp`; it does not
trust that unverified claim for authentication. Asta still verifies the signature. Here the claim
only decides whether spending ten seconds on a refresh is worthwhile, and every malformed shape
chooses the safe slow path.

Three tests cover a week-valid stored token, the exact five-minute boundary, and missing or
non-numeric expiry. A real before/after startup timing still needs the Windows machine with its
signed-in Asta CLI; this headless sandbox cannot start its WSL distro or run the app.

## 146. Left open: stopping a setup repair has two process boundaries (2026-08-09)

§28's cancel remains open, deliberately. A repair on the target platform is usually a Linux
process behind `wsl.exe`; installing WSL itself is an elevated process behind PowerShell and UAC.
This repository already proved that killing `wsl.exe` does not reliably reap the Linux process it
fronted (§26). A button that only drops the receiver or kills the Windows wrapper would say
"cancelled" while `uv sync` or an installer continued changing the machine.

The correct implementation needs a uniquely identified process group inside the distro and a
separate, honest policy for an already-elevated install. That is larger than the three real-machine
defects above and cannot be verified in this sandbox, so no cosmetic Stop button was added.

The transcript virtualization and SVG glyph replacement remain open too. They were explicitly
lower priority, and the former requires a before/after measurement in a real GPUI window. The app
cannot be run here, and replacing a variable-height transcript without that measurement would
repeat §70's mistake in a different type.

## 147. The mirror deleted the checkout it was mirroring into (2026-08-10)

The worst defect this project has shipped, and it ran on every launch for a day.

`sync_source_command` (§139) staged each directory beside its target and swapped it in:

```sh
for d in backend skills; do
  rm -rf $DIR/.$d.new && cp -r $SRC/$d $DIR/.$d.new && rm -rf $DIR/$d && mv $DIR/.$d.new $DIR/$d
done
```

On a real Windows machine `$d` arrived **empty**. So `.$d.new` was `..new`, and `rm -rf $DIR/$d` was
`rm -rf $DIR/` — **the backend checkout, its `.venv`, and `.langgraph_api/checkpoints.sqlite` with
every conversation in it.** Deleted, silently, before the server it was preparing for could start.

What the researcher saw was `backend exited during startup with exit code: 127`.

### Four wrong answers before the log was read

The install failed at `uv sync` with *"No `pyproject.toml` found"*. From reading the script I
concluded `cp -r SRC DEST` had nested the checkout one level deep, wrote that into §141's section,
shipped a fix for it, and told the researcher it was the cause. **It was not.** A later `uv sync`
succeeded at the top level, which proved the directory had never been nested.

Then a diagnostic was handed over that assigned a shell variable inside `wsl bash -lc`. On this
machine those arrive empty — a failure already recorded in this project — so it printed nothing,
and the nothing was read as evidence about the install.

The log had it in four lines the whole time:

```
mv: cannot stat '/home/piero_linux/.local/share/mini-me-desktop/backend/..new'
cp: cannot create directory '.../backend/..new'
bash: line 1: cd: /home/.../backend: No such file or directory
bash: line 1: .venv/bin/python: No such file or directory
```

`..new` is `.$d.new` with nothing in the middle. **The diagnostic that names the failure was
written and shipped and then not read for three exchanges**, which is the actual lesson here — §132
was about a diagnostic that could not distinguish success from failure, and this one could, and it
was still argued past.

### The fix

No shell variable anywhere in the mirror. Two directories and seven files do not need iteration,
and a literal name cannot expand to nothing — so `rm -rf` is now only ever handed
`~/Mini-Me/backend`, never a path that could reduce to `~/Mini-Me/`. The mirror also says so out
loud when the checkout has lost its `pyproject.toml`, because the whole chain is `|| true` and the
damage was otherwise silent until `cd` failed four commands later complaining about something else.

Two assertions now stand where the loop was: that the mirror text contains no `$` at all, and that
no `rm -rf` targets the checkout root. Either would have caught this.

*Why the loop variable is lost is still unexplained. That is precisely why the fix does not use
one: the same machine loses variables assigned in a hand-typed `wsl bash -lc` too, so whatever the
mechanism, it is not something this code should be relying on.*


## 148. An OpenAI default in an app with no OpenAI key (2026-08-10)

A background run finished, and the coordinator said *"completed, but it returned no result text."*
The task had done its work; the answer could not be read back. Behind it, once per poll:

```
GET /threads/019fe9aa-.../state 500 11ms   error_detail=None   response_size_bytes=0
openai.OpenAIError: The api_key client option must be set ...
```

```python
DEFAULT_MODEL_SPEC = os.getenv("MINIME_DEFAULT_MODEL", "openai::gpt-5.4")
```

`backend/models.py` falls back to OpenAI when nothing names a model, and the app **never set that
variable**. Its own comment says exactly when the fallback bites: *"when the request does not
specify one — e.g. assistant schema inspection calls that never execute a node."*
`GET /threads/{id}/state` is one of those, and it is the route the client polls while watching a
background task.

### The invariant that caused it was the right invariant

Provider keys ride in the **run request** and are never written to the backend's environment,
because the agent's own `execute` tool can read that environment (§19). That rule is correct and
stays. But it means a graph built *without* a run has no key at all — and `ChatOpenAI` raises at
**construction**, not at first call. Measured, both ways:

```
openai:gpt-4o             -> RAISES OpenAIError
anthropic:claude-sonnet-4-5 -> constructs fine with no key
```

So the fix is to name the model, not to supply a key: `MINIME_DEFAULT_MODEL=anthropic::…` on the
launch. A model id is not a secret, so nothing about §19 is weakened — and a test asserts the
export carries no `API_KEY=` beside it, so a later convenience cannot quietly undo that.

### What it had been costing, invisibly

- **Every background task's result.** The run succeeded, the poll 500'd, and the coordinator
  reported an empty result — which reads as *"the worker did nothing"*, sending a day of
  investigation at the worker instead of at the read.
- **Old conversations.** `could not read a conversation` in the app log, on the same route.
- **The nested-outputs test**, which could never run, because the background task it needed was the
  thing being lost.

Three symptoms, one line of fallback. The traceback naming it was in the backend log from the first
failure — the same log, the same day, as §147.

*The shape, again, and worth counting: the value the program needed was one it already had. The
researcher had chosen a model in Settings; the run request carried it; the environment did not.*


## 149. Two honest attempts, both blind (2026-08-10)

With §148 merged the background worker finally reported real text and wrote a real file. It also
said *"The plots and summary tables have been saved to files."* There was one CSV.

It had tried. Twice:

```
command failed (exit 1): python -c "... pd.read_csv('/data/potato_late_blight.csv');
                                    ... plt.savefig('/plots/histograms.png')"
command failed (exit 1): python -c "... pd.read_csv('/home/piero_linux/Mini-Me/potato_late_blight.csv')"
```

`/data`, `/plots`, `/home/piero_linux/Mini-Me` — none exist. The workspace was
`/mnt/c/Users/.../Documents/Mini-Me/<thread>`, **commands already run with that as their working
directory**, and `pd.read_csv('potato_late_blight.csv')` would have worked on the first attempt.

### The path was never a secret

`aresolve` announces it — to the *desktop status line*, so the researcher can see where their files
are. The one participant that needed it could not, and the error it got back named the directory it
had invented rather than the one it had. It guessed twice, differently, which is what a model does
when it has no way to find out.

**Sixth time in this document: the thing needed to end the argument was a value the program already
had.** §91's adoptable threads, §99's laid-out width, §110's overlay path, §114's config keys,
§115's work dir — announced to the wrong audience — and now the same work dir again, to the same
wrong audience.

A failed command now carries `[cwd] <path> — this command ran here; use paths relative to it`.
Only on failure: a line appended to every `execute` is a line the model learns to skip, which is
exactly how the corpus-id diagnostic stopped being read (§116/§132).

### The claim is a separate defect and the worse one

Two failed attempts and the turn still reported plots on disk. `exploratory_data_analysis`'s prompt
says *"NEVER invent findings, numbers, or charts"* — §140 has it in the table — and it is the third
capital-letter rule in three days measured at zero compliance. Telling the model where it is may
well stop this instance; it does nothing about the next one, because **the report is not checked
against the workspace at all.**

The structural version is `paper_tools.unreported` pointed at files instead of papers: the workspace
diff already exists (§42 finds figures that way), so a run that claims files which are not there can
be corrected rather than relayed. Left open, deliberately — the cause is fixed, the honesty is not,
and they are different jobs.


## 150. Thirteen files in a directory nobody opens (2026-08-10)

The background worker finally worked. Eight minutes, six plots, seven tables, all correct — and
the researcher could not find any of them.

```
files on disk : Documents/Mini-Me/test subagents/019fe9cb-dbfc-…/   <- the task's id
agent reported: Documents/Mini-Me/test subagents/019fe9c1-e605-…/   <- the conversation's id
Files panel   : the conversation's folder                            <- neither
```

Three places, two directories, no overlap. The work was never lost; it was one directory sideways,
and every surface that could have said so pointed somewhere else.

### The half that worked is what identifies the half that did not

`_forwarded_config` copies `model_config`, `__llm_keys` and `__workspace_project__` out of the
parent run's `configurable`, then separately reads the thread to pin:

```python
pinned = configurable.get(WORKSPACE_THREAD_KEY) or configurable.get("thread_id")
```

The project arrived — the files are under `test subagents/`. The thread did not. **Same dict, two
lines apart.** So this was never "the config did not forward"; it was one key that is not reliably
in `configurable` when a tool call reads it, even though LangGraph's own `pregel/main.py` reads
`saved.config[CONF]["thread_id"]`.

Which version of that is true on a researcher's machine is not something to reason about from here.
`_conversation_thread` now tries the existing pin, `configurable.thread_id`, `metadata.thread_id`
and `configurable.__thread_id__` in turn — **and reports which one answered**, or that none did.

### Why the report matters more than the chain

An unpinned worker does not crash. It creates a real directory, fills it correctly, and reports
paths under a different one. Every signal available to the researcher says success. That is the
same shape as §148's 500 — where a run finished and the answer could not be read — and as §132's
diagnostic that printed identically either way.

A chain of fallbacks that also failed silently would have been the same bug with more code in it.

*Left open beside this: the turn said the plots were saved. It believed that. Nothing checks a
claim about files against the workspace, and until something does, the next one will be wrong in a
way no fallback chain can catch.*


## 151. The folder was never the point (2026-08-10)

§150 pinned a background worker to the conversation's thread, so its files would stop landing under
the task id. The run after it produced ten plots, listed them by name, and the Files panel showed
`provenance.json`.

The researcher's reply reframed the whole thread:

> *"ok it seems it worked but the idea its to somehow view it in the app not as a diferent folder
> outside the conversation folder right?"*

Right. **Three sections have been spent moving files between directories, and not one of them was
the requirement.** The requirement is that a researcher sees what the agent produced without being
told where to look — and every fix so far has been a different way of hoping the file lands
somewhere the panel already reads.

### Why the pin is the wrong shape of fix

It depends on a config key reaching a tool call. When it does not, the failure is silent by
construction: a real directory is created, filled correctly, and reported under a different id, so
every signal says success (§150). A fallback chain makes that less likely and cannot make it
impossible, because the app is still only ever looking in one place and guessing that the worker
agreed.

The version that cannot fail that way is the opposite one: **the app reads the finished task's own
folder.** It knows the task id — it displays it, polls it, and prints it in the answer. A worker
that writes anywhere reachable is then visible, whether or not the pin worked, and the pin becomes
an optimisation rather than a load-bearing guess.

That is the §91/§115 shape one more time, and worth counting because it is now seven: *the value
needed was one the program already had.* The task id is on screen.

### Both, not either

Keep the pin — files beside the conversation is the right filing, and the log line it now emits is
how we learn whether the key ever arrives. Add the read — because a researcher who cannot see their
own plots does not care which of the two failed.

*Left open, and the harder one: the run that listed ten filenames believed it. Nothing checks a
claim about files against the workspace, and until something does, "I saved the plots" is a
sentence the agent can produce whatever happened.*


### The answer was one directory deeper, not one directory over

The pin (§150) worked on the next run — `background work pinned to 019fe9d7-… (from
configurable.thread_id)` — and the researcher supplied the design the three previous attempts had
all missed:

> *"from thread lets say A I send the background task. Then the subagent created a subfolder B and
> the files were in B not in A. When B must be inside A."*

Not *instead of* A. **Inside** it.

```
Documents/Mini-Me/<project>/A/        the conversation
Documents/Mini-Me/<project>/A/B/      the background task it started
```

`workspace::outputs` already descends through named subfolders and shows the relative path — that
was §143, built the same day for an unrelated reason. So nesting makes the worker's files appear in
the conversation's Outputs panel **with no client change at all**, labelled `B/plot_yield.png`, and
*which run produced them* survives.

Writing straight into the conversation's folder — which is what the pin alone did — would have
shown the files and destroyed that, mixing every worker's output together with the conversation's
own. The nesting keeps both.

`LocalSandbox.__init__` composes the path from parts now: `[pinned]` when a run is its own
conversation, `[pinned, own]` when it is not. An unpinned worker still gets its own folder, because
a failed pin must cost discoverability and never the files.

*Three sections of moving files sideways, and the fix was to go one level down. The researcher saw
it in one sentence.*


### Two directories for one task

Nesting shipped, and the next run produced **both**:

```
…/test subagents/019fe9ed-bb8a-…/019fe9ee-5b78-…/   created, and empty
…/test subagents/019fe9ee-5b78-…/                   a sibling, holding all ten plots
```

The same task, filed twice. Which means the sandbox is constructed **more than once per run**, and
`get_config()` carries the pin at one site and not at another:

```
pinned:   parts = [conversation, task]  ->  …/conv/task/   (created, empty)
unpinned: parts = [task]                ->  …/task/        (gets the files)
```

From the outside that is indistinguishable from "the nesting did not work" — and it is §123 again,
where a `ContextVar` store did not survive a task boundary. The answer is the same one: keep the
fact where the process shares it. `_PINNED_BY_THREAD` remembers the conversation the first time we
are told, keyed by the task's own id, which is unique to one background run and so cannot mis-file
one conversation's work under another's. A later pin that *disagrees* is logged rather than
honoured, because a thread belongs to one conversation for its whole life.

*Eighth time the value needed was one the program already had — and the second time in this thread
that it was held somewhere a later caller could not reach.*


## 152. Twelve rows that all begin the same way (2026-08-10)

It works. A background worker generated a dataset, analysed it, drew ten figures, and the researcher
saw them without being told a path:

    019fe9f6-9126-7710-a806-35d5e09170a4/guinea_pig_eda_output/plots/health_by_activity_box.png

And the panel showing them is now the problem. Twelve artifacts, twelve rows, every one truncated to

    019fe9f6-9126-7710-a806-35d5e09170a4\guin…

**The characters they share are displayed; the characters that distinguish them are cut off.** A
list where every row reads the same is not a list, and this is the §59 failure in a new place — a
label that collapses under its own length — arriving because the workspace finally has enough in it
to expose it.

The transcript has the matching problem: every figure renders at full width, so a productive run is
ten screens of scrolling past pictures the researcher has not chosen to look at yet.

### What the researcher asked for

> *"when we have too many plots and table maybe doing something like whats do with too many photos.
> Group them and only when click you can view it and scroll in x axis."*

A gallery. Grouped, collapsed to thumbnails, opened on click, scrollable sideways.

**And the grouping already exists in the data.** The agent organised its own work into
`guinea_pig_eda_output/plots/` — §143 taught the panel to descend through exactly that structure
and then flattens it back into one list. The folder the agent chose is a statement about what
belongs together, and the panel is discarding it.

*Ninth: the value needed was one the program already had.*


## 153. The folder becomes the gallery (2026-08-10)

§152 did not need an invented grouping model. The recursive output walk already retained the
relative parent of every artifact; the two renderers were simply throwing that boundary away.
They now group on the **full parent path**, so two different workers' `plots/` directories cannot
silently merge, while the heading removes only a leading generated thread UUID and says the part
the agent chose: `guinea_pig_eda_output / plots`.

The interaction follows the platform's own collection pattern rather than copying WhatsApp's
decoration. Microsoft's Windows guidance names interactive photo libraries as an ItemsView use
case and puts scrolling inside the collection
([Items view](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/itemsview)). In
GPUI that becomes one fixed-size thumbnail rail per folder, horizontal overflow, a visible
horizontal thumb, and the existing preview modal on click. A folder with one artifact keeps its
larger card; collapsing it would save no space and make an ordinary one-file answer worse.

Both surfaces use the same grouping rule:

- In the transcript, a productive folder is one rail instead of one full-width card per file.
- In Outputs, the same folder is one compact rail instead of twelve near-identical rows.
- A tile's primary label is the basename, never the shared relative path. If the basename itself
  is too long, its **leading** edge is elided so the differentiating tail and extension survive.
- Every rail owns its own `ScrollHandle`; moving one folder cannot move another. The bar is drawn
  explicitly because GPUI's overflow scrolls but supplies no visual affordance, and a clipped row
  on a mouse-driven Windows desktop does not communicate sideways content.

The structural before/after is deterministic even in the headless build environment: the reported
ten-plot turn built ten figure cards, each allowed up to 420px of image height; it now builds one
folder block whose figures share a single 118px-high rail. The exact rendered pixels still need a
Windows-eye check because this machine cannot open GPUI. Two sentence-named tests pin the folder
boundary, UUID removal and distinguishing filename tail. The complete result is **241 passing
tests**, with no new Clippy warning (the base branch's existing warnings remain).

*Tenth: the value needed was one the program already had — and this time the implementation keeps
it instead of flattening it twice.*


## 154. A project existed in two places (2026-08-10)

The report was two symptoms of one contradiction: *new conversations already start in a project*,
and *a deleted project comes back after launch*.

§106 says a project is exactly a name carried by at least one conversation — no project registry,
because a second truth will drift. But §107 added `settings.project`, remembered the last project
opened, restored it on startup, and deliberately inherited it for ordinary **New thread**. An empty
project therefore still existed in settings after its last conversation left the sidebar, and the
next launch put fresh work back under that name. The persistence was not hidden in the backend;
it was a second client-side registry this plan had explicitly ruled out one section earlier.

The related upstream report, `docs/upstream/mini-me/project-spine-is-not-per-project.md`, identifies
a different boundary: the spine route is server/user-scoped rather than project-scoped. That can
make project *content* look shared, but it does not create the sidebar heading or choose the
workspace directory. Treating it as this resurrection would have changed the protected backend
and left the actual `settings.toml` value untouched.

There are now only two ways to choose a project:

- Open a conversation already filed there.
- Click the `+` beside that project heading to start a conversation there deliberately.

Launch and ordinary **New thread** both choose the workspace root. The old `project` settings key
is accepted and ignored when upgrading, then disappears on the next save. Root conversations are
labelled **Ungrouped Conversations** in the sidebar and picker — still `None` in metadata and still
directly under `Documents/Mini-Me`, so the friendly name does not become a third registry or a
real folder that can collide with one the researcher creates.

### Why deletion also had to change

A project heading is derived from its conversations, so deleting the last one is deleting the
project from the app. The row used to disappear and say *conversation deleted* **before** the
HTTP delete answered. `Sidecar::delete_conversation` then ran fire-and-forget; if the request
failed or the app closed first, the durable thread remained and the next listing correctly brought
it — and its project — back. The UI had reported an operation that had not happened.

Deletion now leaves the row in a **Deleting…** state and removes it only after a successful server
answer. Failure or a dropped answer keeps the conversation visible and says it was not deleted.
Deleting the open conversation also clears the sidecar's project, so the empty slate cannot inherit
the project whose last durable member just went away. Files and empty project folders in Documents
are deliberately not deleted: they are researcher-owned outputs, and §58's delete contract has
always promised to leave them alone.

Two sentence-named tests hold the upgrade and failure paths: an older remembered project is ignored
and not written back, and every non-successful delete resolution keeps the row. The window still
needs the Windows restart check; this headless environment can prove the state transitions and wire
result, not watch the heading disappear and stay gone across two launches.

*The eleventh value the program already had was `None`. Remembering more state was the defect.*


## 155. Deleting the label left the laboratory behind (2026-08-11)

The Windows check of §154 found the other half immediately: the project stopped returning to the
sidebar, but its conversation directories and every file in them remained under
`Documents\Mini-Me`. The plan said that was deliberate in §58 and repeated it in §154 — files were
the researcher's, therefore deletion must leave them alone. The researcher changed that contract
after seeing it operate: *"We need to sync that."*

That is the better rule now that §105 made projects real folders. A conversation in the sidebar and
its directory in Explorer are two representations of the same work. Deleting one while preserving
the other is not caution; it is an orphan that looks like the deletion failed. The confirmation is
where caution belongs.

### The warning moved to the centre because its scope grew

The old confirmation replaced a sidebar row with *Delete this conversation? · delete · keep*. It
could fit a noun and two verbs. It could not honestly say that the chat history **and every saved
output** were going, show the exact directory, or explain that deleting a project also removes files
placed directly in its folder. A second click without those consequences stated is not informed
confirmation.

Deletion now uses the existing centred `Modal`, with pinned Cancel and red Delete actions and an
explicit *There is no undo*. A conversation warning names the conversation and its exact folder. A
project warning names the project, counts every conversation, shows the project path, and says that
the entire directory goes — including files Mini-Me did not create.

### A project has a delete control of its own

Named project headings now appear even when there is only one group. §106 hid a single heading as
noise; it is no longer decoration once it owns **open folder**, **new conversation here**, and
**delete project**. The delete control targets the complete project from `self.conversations`, never
the rows surviving the sidebar search — filtering for one title must not turn "Delete project"
into "delete the one conversation I can currently see". Ungrouped Conversations is still not a
project and therefore has no project-delete control; its conversations can be deleted individually.

### The two irreversible systems cannot be atomic

LangGraph and NTFS cannot share a transaction. The order is therefore server first, filesystem
second:

1. Delete every durable thread and keep the rows in **Deleting…** state while that happens.
2. Only after those requests succeed, recursively remove the confirmed conversation or project
   folder on a blocking worker, never on GPUI's render thread or Tokio's reactor.
   A currently open target cannot be deleted while its foreground turn or background work is still
   writing there. **Only the open one**, and that is a real limit rather than an oversight: the
   workbench holds `tasks` for the conversation on screen and clears them when another is opened,
   so it cannot know that a conversation it is *not* showing has a worker still running. Delete
   that one and its tree goes while the worker writes into it — the server thread is already gone,
   so the worst case is a folder that reappears owning nothing, which is visible and recoverable
   rather than silent. Closing it properly needs the backend to report a conversation's live
   workers, which is the same missing fact §42 wants for "a run claims files it never wrote".
3. If the server refuses, preserve the files and refresh the list; a project batch may have stopped
   after earlier threads succeeded.
4. If Windows has the folder open and cleanup fails, remove the now-nonexistent conversation from
   the sidebar but report that its recoverable folder remains. Restoring a row whose server thread
   is gone would be another lie.

The destructive path validates the thread id as one path component before recursion. It refuses to
descend through a project symlink or junction, and unlinks a target link rather than following it;
one malformed server id or an Explorer shortcut must never widen one confirmation beyond the
Mini-Me workspace. Deleting one conversation removes its nested output tree and then its project
directory only when empty. Deleting a project removes the complete named directory.

Four sentence-named filesystem tests pin the boundaries: nested files go with their conversation,
a neighbouring conversation survives, the last conversation removes an empty project, a whole
project deletion leaves its neighbour alone, and hostile thread ids cannot escape the workspace.
UI tests keep a failed durable delete visible, keep a folder-cleanup failure from resurrecting a
deleted thread, prove a project target contains conversations hidden by search, and end the active
project when its final conversation disappears.

This explicitly supersedes §58 and §154's decision to preserve output files. The warning makes the
new deletion contract visible before the click rather than leaving the cost behind afterward.

Headless verification on the Windows checkout: **248 tests pass**. `cargo clippy
-p mini-me-desktop-app --all-targets` adds no warning; it still reports the branch's existing 15
warnings in untouched call sites. The modal itself still needs the Windows-eye check — specifically
its path wrapping and the two hover controls on a single-project heading.
## 156. A transcript row should not exist until the viewport needs it (2026-08-10)

The transcript still did this on every render:

```rust
for message in self.transcript.iter() {
    // build the entire GPUI element tree
}
```

Markdown parsing was no longer in that loop — §70 already moved the expensive parse to the one
message whose body changes — so claiming virtualization would make the app faster without measuring
would repeat the plan's original mistake. The unit that can be measured honestly in a headless TTY
is **row-construction calls per frame**. With 500 alternating 36px/180px rows in a 600px GPUI test
window, the eager loop constructs **500 of 500**; `gpui::list` with 240px overdraw constructs **15 of
500**. This is not a wall-clock claim. It proves the allocation/layout work was removed from the
off-screen 97%, while acknowledging that cached Markdown parsing was probably the larger win.

`uniform_list` is wrong here. It assigns every row the first row's height, while this transcript
ranges from a one-line question to a report several pages long. The workbench now owns a
`ListState`, renders with GPUI's variable-height `list`, and splices only the in-flight message or a
trace whose disclosure changed. The live status line is another row. The visible scrollbar remains;
`ListState` exposes different offset metrics from `ScrollHandle`, so it has a matching small helper
instead of a second scroll container fighting the list.

Virtualization exposed a less obvious dependency: §62's selection registry used paint order as
document order, and Select All discovered its text from the spans painted that frame. That would
make row identities shift during a scroll and would copy only the viewport. Selectable runs are now
keyed by `(message, run)`, text from visited rows outlives their layout rectangles for drag-copy, and
Select All builds a logical plain-text transcript from the already-cached message blocks. A test
proves an unpainted Markdown message still copies the same rendered words, not Markdown punctuation.

The headless tests establish variable heights, the 500-to-15 construction count, stable span order,
and off-screen Select All. They cannot establish wheel feel or visual scroll anchoring on the target
machine; that remains a short Windows app check after the branches are merged.

## 157. Four glyphs become four packaged icons (2026-08-10)

The deferred §70 item was kept deliberately narrow. Settings, the conversations toggle, the
research toggle and the command palette's Enter hint now use four hand-authored 24×24 SVGs. Their
labels, hit targets, colours and surrounding layout are unchanged; this is an asset substitution,
not another navigation redesign.

The first literal set — gear, one speech card, paper under a magnifier — was technically clear and
looked like every utility application. The approved set shares a **research atelier** language:
Settings is a calibration instrument, Conversations is two distinct voices facing opposite ways,
Research is an open field notebook with discovery outside its pages, and Enter is one curved ink
stroke. Small solid points and the same rounded 1.55 stroke tie them together without adding detail
that disappears at the 14px size where they actually render.

GPUI resolves `svg().path(...)` only through an `AssetSource`. Reading an `assets/` path at runtime
would work in a checkout and fail in the installed Windows executable, so the source maps four
known paths to `include_bytes!` data compiled into the binary. Every stroke uses `currentColor`,
which lets the existing semantic text colour and hover state tint it. A test loads every declared
path through the same source, checks the shared view box and tint token, and proves an unknown path
returns absence instead of a misleading asset.

The identical glyphs used as data/file-type marks and in the empty-state suggestions were not
changed. They describe different concepts and replacing them would widen a four-control cleanup
into an icon-system redesign — exactly the scope this deferred item said to avoid.

### Caught in review: they rendered nothing

`app_icon` was `svg().path(p).w(14).h(14).flex_none()` — no colour. GPUI paints an SVG by
rasterising it to a mask and multiplying by the element's text colour, and `Svg::paint` is
literally:

```rust
if let Some((path, color)) = self.path.as_ref().zip(style.text.color) { … }
```

`None` there paints nothing. And that colour is **not inherited**: `compute_style` starts from
`Style::default()`, whose `text.color` is `None`, and refines it with the element's *own* styles —
so the `.text_color(…)` each call site sets on the surrounding row never reached the icon inside
it. All four were invisible.

`ink` is a required argument now, so the compiler refuses a call site that forgets. That is the
right instrument here, because the alternative is a rule written down, and this file already
records what happens to rules written down (§59, three times).

### The test could not have caught it

It asserted the SVG source contains `currentColor` and called the icon *"tintable"*. GPUI never
reads `currentColor`; usvg resolves it while rasterising and the result is a mask. So the
assertion passed identically whether the icons rendered or not — the shape this project keeps
finding, a diagnostic that prints the same for success and failure (§38, §148, §161, §166).

It now claims only what it settles: the bytes are embedded, every declared path resolves, and an
undeclared one does not. Whether an icon *appears* is a fact about the call site, and the type
system holds that one.

**Still unverified:** whether the four drawings read at 14px on both palettes. That needs eyes on
a window and this build machine has none.
The state existed on disk the whole time; the sidebar was reading the wrong evidence for it.*
## 158. A scrollbar that only looked interactive, and an image cache that remembered too soon (2026-08-11)

The first Windows pass on §153 found both failures immediately. The agent generated the figures,
their cards appeared, and the pictures themselves stayed blank until the application restarted.
The horizontal thumb was visible beneath them, but clicking and dragging it did nothing.

These were two different stale-state mistakes:

- The Outputs panel scans the filesystem whenever it paints, including while an `execute` process
  still has a PNG open. GPUI's global image cache keys a local image by path and retains a decode
  error just as it retains a decoded image. If the first read lands between create and close, every
  later frame asks for the same path and receives the same cached failure. Restarting worked only
  because it rebuilt that cache. A finished foreground turn or background task now schedules two
  bounded follow-up passes across the Windows/WSL hand-off. Each pass re-collects late files,
  evicts figure paths from GPUI's asset cache, and repaints. This is deliberately not a permanent
  watcher: outputs are bounded completion events, and polling the researcher's Documents folder
  forever would spend idle time fixing a race that only exists while a writer is finishing.
- `horizontal_scrollbar` was a six-pixel painted `div`, not a control. It communicated the native
  scrollbar contract without implementing it. The gallery now gives the bar a 12px mouse target,
  maps the thumb's travel onto the `ScrollHandle`'s hidden width, preserves the point grabbed
  inside the thumb, supports clicking the track, and ends the drag even when release is observed
  away from the thumb. Each rail still owns its own handle, so §153's independent positions remain.

The mapping is a pure function with a sentence-named regression test: the left edge produces zero
offset, the midpoint reveals half the hidden width, and dragging beyond the right edge clamps at
the last file. The image-cache correction still needs the same Windows-eye check that found it:
generate several plots, leave the app open, and confirm the thumbnails fill without a restart.

*Eleventh: an affordance is a promise of behavior, and a cache key needs the version of the thing
it remembers—even when the library only gives us its path.*


## 159. The client knew the parent; the worker was asked to guess it (2026-08-11)

A second live EDA exposed the same filing defect §150/§151 had made less likely, not impossible:

```
Documents/Mini-Me/019ff21e-a473-…/   the conversation
Documents/Mini-Me/019ff231-2332-…/   its delegated worker, beside it
```

The worker completed and the answer rendered, but the conversation's Outputs panel contained only
its own two bookkeeping files. The researcher deleted that reproduction before it could be read
from disk; the exact UUIDs and timestamps remain in the screenshot, while both backend routes now
correctly return 404 for the deleted threads. A following run landed correctly. That difference is
the evidence: this is an intermittent ownership signal, not a deterministic path calculation.

### The request already had the only authoritative value

Every coordinator request is sent to `/threads/<conversation>/runs/stream`. The Rust client
therefore knows the conversation id at the point it assembles the run config, but sent the model,
keys and project without `__workspace_thread__`. The overlay then tried to reconstruct the missing
owner inside `start_async_task` from, in order, an inherited pin and three LangGraph thread metadata
locations. §150 already measured why that cannot be load-bearing: not every tool-call context has
those metadata fields. When none does, a valid worker starts with its own UUID as the workspace
root; no error occurs and every generated file is filed one directory sideways.

The client now sends `configurable.__workspace_thread__ = <conversation>` on every fresh turn and
foreground resume. The async launcher already gives an explicit pin priority and forwards it under
its directory-only key, so the background thread still owns its checkpoints while its files are
nested under the conversation. No protected overlay code needed to change.

Background approval resumes carry the owner again. This closes a second version of the same race:
the overlay remembers worker→conversation ownership in process memory, but a backend restart while
a task waits clears that map. A decision made afterwards must not let the rest of the task resume in
a sibling folder.

### The resume asked the wrong object which conversation it was

Caught in review, before the change shipped. The resume path read the owner from
`Sidecar::thread_id()` — *the conversation open right now* — and those are not the same thing.
`open_conversation` clears the task list, so switching conversations is safe; `Command::NewThread`
never did, so a pending task from the previous conversation stayed on screen and stayed clickable
while `thread_id()` moved on to a new thread. Approving it then named the new conversation as the
worker's owner.

With the backend still running this was invisible: `_PINNED_BY_THREAD` already held the true owner,
first-sighting-wins kept it, and the disagreement was logged. With the backend restarted — the one
case this whole change exists for — that map is empty and the new conversation wins. So the fix was
inert exactly where it was safe and wrong exactly where it mattered, and its failure mode was
*worse* than the one it replaced: a sibling UUID folder is visibly wrong, while files appearing
inside an unrelated conversation's Outputs panel look like they belong there.

The owner is now carried on `AsyncTask`, stamped by whichever call site ingested it — the streaming
snapshot (safe to read the open thread there: `apply` only runs mid-turn, and both New thread and
opening a conversation refuse while streaming) or `open_conversation`'s own parameter. A task
already being watched keeps the owner it was first seen with. `owning_conversation()` puts the
"blank is not a directory name" rule next to the field instead of at the call site, because an
empty pin would write to the workspace *root* — strictly worse again. Unknown sends no key at all
and lets the backend fall back to its own inference. `Command::NewThread` now clears `tasks` and
`jobs`, which is the pre-existing bug this change had made load-bearing.

Three sentence-named tests: blank and whitespace owners name no conversation, the payload decoder
does not invent one, and the request shape covers fresh runs, resumes and blank ids.

The live Windows confirmation is deliberately exact: start a new conversation, ask it to delegate an
EDA, and leave Explorer open at `Documents\Mini-Me`. The correct result is one new top-level
conversation UUID and the worker UUID, if it creates one, **inside** it—never a second UUID beside
it. Then the harder half: with a task waiting for approval, press New thread. The pending card
should disappear with the conversation it belonged to.

*Twelfth: a value known at the boundary should cross the boundary explicitly, especially when its
absence is a successful-looking failure. And the corollary the review found — the boundary has to
send the value that belongs to the **work**, not the one that belongs to the window.*
## 160. Sixteen real files escaped to WSL `/tmp` (2026-08-11)

This first looked like §150 again: Explorer showed two UUID folders under `Documents\Mini-Me`
after one EDA. They were not duplicates. The backend store identified them as two independent
conversations:

```
019ff236-183c-…  14:04  earlier penguin conversation
019ff25d-f0ee-…  14:47  current Coffea arabica conversation
```

The current run used the ordinary synchronous `task` tool, had no `async_tasks`, and every one of
its three run/resume requests stayed on `019ff25d-f0ee-…`. There was no background-worker UUID to
pin. Its empty Outputs panel was a different failure.

The coordinator explicitly asked `exploratory_data_analysis` to *"save outputs in the working
directory using relative paths."* The subagent reported sixteen paths such as
`tmp/coffee_eda/coffee_arabica_dummy_dataset.csv`, while the conversation's stored
`artifacts.files` was empty and the directory contained only `memories/` and `provenance.json`.
The files did exist, byte-for-byte, here:

```
/tmp/coffee_eda/
├── coffee_arabica_dummy_dataset.csv       46,772 bytes
├── eda_*.csv / top_correlations.csv         7 summary CSVs
├── fig_*.png                                7 figures
└── eda_notes.txt
```

All sixteen timestamps fall between 14:49:30 and 14:50:05, inside the measured tool run. This is
not hallucinated output. It is successful work written to WSL's disposable global temp directory,
where Explorer, `workspace::outputs`, artifact capture, conversation deletion and project moves
cannot see it.

### The two instructions disagree, and `execute` makes the dangerous one real

The installed DeepAgents `EXECUTE_TOOL_DESCRIPTION` tells the model:

> *"Try to maintain your current working directory throughout the session by using absolute paths
> and avoiding usage of cd."*

Its examples are `/foo/bar` and `/path/to/script.py`; it does not name this run's actual work
directory. The task's request for relative paths therefore loses to a filesystem system prompt
that says absolute paths are the convention, and `/tmp` is a plausible guess for an isolated
sandbox.

The enforcement gap is in `overlay/minime_local/workspace.py`, at
`LocalWorkspaceBackend.aexecute`, not in the Rust Outputs walk and not in background pinning:

- `LocalWorkspaceBackend` deliberately uses `virtual_mode=False`, so file operations and executed
  Python share one real path namespace (§18).
- `_reroute_write` safely re-roots absolute paths used through `write` and `upload_files`.
- Shell/Python execution is different: `aexecute` runs with the conversation as `cwd`, but an
  absolute `/tmp/...` remains an absolute host path. §18 records this as merely human-gated; the
  approval gate controls whether a command runs, not where it writes.
- Artifact capture scans the conversation work directory. An escaped file is correctly absent
  from `artifacts.files`, which is why both the transcript gallery and Outputs stay empty.

### Fix it at the execution boundary

The durable invariant is: **a command may read an explicitly named external input, but every
persistent output it creates belongs below `LocalWorkspaceBackend._work_dir`.** The local backend
owner should implement and test that invariant around `aexecute`:

1. Give the local-mode execute tool an instruction that names the real `aget_work_dir()` and says
   persistent outputs must use that directory or paths relative to its `cwd`; `/tmp` is explicitly
   ephemeral and outside the app.
2. Add enforcement, not only prose. At minimum, refuse obvious persistent writes to `/tmp` and
   return a tool error naming the current work directory so the model can retry correctly. The
   complete version isolates execution so the workspace is the only writable persistent mount, or
   bind-mounts the run's `<work_dir>/tmp` over `/tmp`. Do **not** try to understand arbitrary shell
   syntax with a regex and call that containment.
3. Add a cross-layer regression test that runs Python writing
   `/tmp/minime-escape/result.csv` and proves either the command is refused or the bytes appear at
   `<work_dir>/tmp/minime-escape/result.csv`, never in WSL's global `/tmp`; then prove artifact
   capture returns that file.
4. Keep external reads working. Researchers intentionally attach datasets outside the workspace
   (§28), so making the whole filesystem unreadable would fix outputs by breaking inputs.

Three tempting fixes do not close this bug: `TMPDIR=<work_dir>/tmp` does not affect a literal
`/tmp/...`; `virtual_mode=True` does not constrain `execute` (§18); and copying paths mentioned in
assistant prose would let untrusted text ask the desktop client to import arbitrary host files.

Until enforcement ships, the current sixteen files can be preserved by copying
`/tmp/coffee_eda/` into `Documents\Mini-Me\019ff25d-f0ee-…\coffee_eda\`. Copy, do not move: the
diagnostic reproduction should remain intact until the backend fix is verified.

*Thirteenth: a working directory is a default, not a boundary.*


## 161. Advice, where a boundary was wanted (2026-08-11)

§160 found sixteen real files in WSL's global `/tmp` and located the cause precisely. Both of its
load-bearing claims verify against the pinned package: `filesystem.py:422` carries *"Try to
maintain your current working directory throughout the session by using absolute paths and
avoiding usage of cd"* verbatim, and `_reroute_write` is called from `write` and `upload_files`
and **nothing else**, so `aexecute` has never been re-rooted.

The description is now rewritten before any middleware is built. The sentence is replaced, and a
rule is appended that names the consequence in terms the model can act on — *"do not appear in the
researcher's Outputs panel, are not kept when the conversation is filed or deleted, and are erased
by the operating system"* — rather than the useless abstraction *"outside the workspace"*. Reading
an absolute path stays allowed: a researcher attaches datasets from anywhere (§28), and a rule that
forbade absolute reads would fix outputs by breaking inputs. Upstream's own guidance about `&&`
versus `;` is left exactly as written; this replaces one sentence, not a document somebody else
maintains.

### What it is not

**It is advice.** §160 proposes, as a fallback, refusing "obvious persistent writes to `/tmp`" —
and in the same section warns *"do not try to understand arbitrary shell syntax with a regex and
call that containment."* Both cannot stand. A command is an arbitrary program; pattern-matching it
produces a claim that is false in every case nobody thought of, and **a false boundary is worse
than a documented absence of one**, because the next reader stops looking.

So the boundary stays open and stays recorded as open. Real containment is a bind-mount of
`<work_dir>/tmp` over `/tmp`, or an execution namespace where the workspace is the only writable
persistent mount. That is a larger change than a docstring and worth making; it is not worth
pretending to have made.

### Patched by name, and honest about it

`create_deep_agent` takes no `custom_tool_descriptions`, and `FilesystemMiddleware` reads the
module global when it builds the tool (`filesystem.py:1481`), so replacing that global before any
middleware is constructed is the reachable point. Patching a third party by name is one of this
project's recurring bug shapes, so the replacement targets an exact sentence and **logs either
outcome**: if upstream rewords that line, the log says the advice it contradicts may have returned,
instead of reporting success over a no-op. §132's rule, applied to our own patch.

*Fourteenth, and §160's own sentence is the right one: a working directory is a default, not a
boundary.*


## 162. Four tiles, and the fourth one counts the rest (2026-08-12)

§153 gave every folder a sideways strip, which fixed the ten-screens problem and left two the
researcher named as soon as they saw it on their own data:

> *"I want to group images and in another group other files. So when the user clicks a modal
> appears and we can click and scroll at the bottom so the user can select which picture to see."*

Their reference was explicit, and a phone: a 2×2 photo grid whose fourth tile reads `+5`, opening
into a viewer with arrows, `1 de 8`, and a filmstrip along the bottom with the current frame
outlined.

### Kind outranks folder on the surface you flick through

§153 grouped by the directory the agent chose and was right about structure. It was wrong about
kind: the run that prompted this wrote seven figures and a summary CSV into one folder, so the CSV
sat in the middle of the strip you scan looking for a plot. `split_images` now puts figures in one
group and everything else in another, images first, because a figure is what the panel is opened
to *look at* while a CSV is opened to check something.

Folder grouping is kept for the non-image group. Two runs' `results/` directories are still two
things; the image grid is the one surface where kind wins.

### The preview had nothing to go next to

`preview: Option<workspace::Output>` held a file, so choosing among eight figures meant closing the
modal, finding the next thumbnail and opening it again. It now holds a `Preview` — the set and a
position in it — which is the single fact the arrows, the `3 of 8` counter and the highlighted
filmstrip tile all render. §158's rule about one calculation, applied to three affordances that all
mean *this one*.

`Preview::opening` is the only constructor and returns `Option`, because `current()` indexes: a
click can arrive after the files behind it were moved, and §159's own reproduction was deleted
mid-diagnosis. Out-of-range clamps, empty is `None`, and stepping wraps — the counter says which of
how many, so wrapping cannot be misread as a dead button, and comparing the first plot of a series
with the last should not mean travelling back through the middle.

### The `+N` was wrong and the test said so

The first implementation counted `total - tiles`, which for eight images in four tiles reads `+4`.
The phone reads `+5`. The scrimmed tile is *covered*, so it counts among the hidden: three pictures
you can see, five you cannot. `image_grid_shape` is one function used by the grid and by its test —
not two copies of the rule — and the test asserts the property the off-by-one broke, that at every
size every image is either visible or counted. It failed on the first run and named the number.

No arrow-key bindings, deliberately. Focus lives in the composer, an unscoped `left`/`right` would
take the arrows away from typing, and a scoped binding never fires from there — the trap §58 and
§84 already paid for twice. The modal is clickable and Escape already closes it; keyboard
navigation wants a focus handle on the modal and is its own change.

*Fifteenth: when someone hands you the thing they want copied, copy the arithmetic too — `+4` is
defensible in review and wrong beside the screenshot.*


## 163. Painting over something is not being in front of it (2026-08-12)

Three defects from the first real look at §162, and the third is the one worth the section.

### The arrows closed the modal they were stepping through

Click handlers fire on GPUI's **bubble** phase, innermost first, and every one on the path runs
unless something stops it. The arrows sit inside the panel, the panel sits inside the backdrop, and
the backdrop closes the preview on click. So a press on `›` stepped forward and then closed.

`stop_propagation` on the panel, not on each control: the same path was already reachable from
`open outside` and from every filmstrip tile, and one guard at the boundary covers the controls that
do not exist yet. This was live for the header buttons before §162 and nobody had clicked one.

### The picture was cut off at the top

`img().max_w_full().object_fit(Contain)` bounds one dimension. The height resolved to the file's
own, which for a tall plot exceeded what the flex row allowed, and `items_center` then clipped it
symmetrically — the top of a stacked bar chart gone, dead space underneath, and `Contain` never
getting the chance to letterbox because there was no box to contain it in.

Both dimensions are now set, on a named constant, and the body has its own ceiling: a child with
`overflow_y_scroll` needs a bounded height to scroll *within*, or the clipping simply moves
somewhere else. The modal also went from 760px to 880px wide — 760 was chosen when this previewed
CSV rows, and it is narrow for a figure with five rotated category names on the x axis.

### Every overlay in the app was click-through

The researcher's words, and they generalised it themselves before I did:

> *"If I click something in a modal and in the background there is something clickable, the
> interaction in the background happens. It means that if two buttons overlap, and I click the
> button in front, both buttons activate."*

GPUI hit-tests **every** element whose bounds contain the pointer. Painting later puts you on top
visually and does nothing to the hitboxes underneath, so a dimmed backdrop was a picture of
exclusivity rather than exclusivity. `occlude()` sets `HitboxBehavior::BlockMouse`, which is the
thing that was missing — from the preview backdrop, the command palette, the toast stack, and
`ui::Modal`, which is to say from Settings, About and Provenance at once.

Fixed in `ui::Modal` rather than in its three callers, because all three had the same defect for the
same reason and a fourth modal would have inherited it.

The context menu is the tell that this was known and half-solved. It already carried:

```rust
// Swallow the press so the click that chooses an item does not also land on the
// transcript underneath and start a fresh selection there.
.on_mouse_down(gpui::MouseButton::Left, |_event, _window, cx| cx.stop_propagation());
```

A press guard, written against one observed symptom. A click is a press *and* a release, so the
release still reached whatever was behind — and no other overlay got even that much. The press
guard stays, because it also stops the drag a mouse-down on the transcript begins; `occlude` is the
general form of what that line was reaching for.

Not verified here: all three are GUI behaviour on a headless machine. The arithmetic has constants
and the occlusion has a named API, but whether the picture now fits and whether the arrows now step
is something only running it can say.

*Sixteenth: z-order is a fact about drawing. Reachability is a separate fact, and it has to be
stated separately.*


## 164. The strip was the wrong shape, and one renderer is enough (2026-08-12)

§162 worked, and looking at it against the thing it was imitating showed two more:

> *"For files we should do the same. Also I think the grouping occupies too much space in the
> conversation (too wide). Check the screenshot from WhatsApp. Is less invasive and functions the
> same."*

### Fixed tiles, not fractions

The image grid used `flex_1` tiles, so the block was as wide as whatever held it — in the transcript,
the whole conversation. A folder of seven files claimed a band wider than the answer that produced
it, which is the §152 complaint in a new direction: not ten screens tall, one screen wide.

The phone gallery it is compared against is a *block* — about 415px, the same whatever the chat
window is doing. So tiles are now a fixed width and there are two per row, which makes the grid
exactly `2 × tile + gap` and no wider: 304px in the panel, 408px in the transcript. There is a test,
because "too wide" is the complaint and a `flex_1` slipped back in would look correct in review.

### One renderer for images and for files

§153's sideways strip is gone. It had a heading, a `scroll sideways` hint, a visible scrollbar, and
a full-width band per folder — furniture, for three CSVs. Files now use the same capped grid as
images, so they get the `+N` tile and the modal with it, and the strip's scroll machinery lives on
where it is actually wanted: the preview filmstrip, which is the surface §162 gave it.

Two differences inside a tile, and they are the whole reason one renderer works. A figure shows the
picture and **no filename** — the image identifies itself, the modal's header names it, and a caption
under every thumbnail was half of what made the strip feel heavy. A data file shows a glyph, the
name and the shape, because one CSV looks exactly like another.

`Contain` rather than `Cover` for a tile's picture: a photo crops acceptably and a chart does not.
Cropping the axes off a plot makes the thumbnail useless for the only job it has, which is choosing
between seven of them.

### §59, third time

Every file tile in the panel rendered its name as a bare `…`. `Label::ellipsis` produces
`flex_grow().min_w_0().truncate()`, `flex_grow` needs a flex parent to grow within, and GPUI's `div`
defaults to `Display::Block` — so a tile's label row was a block container and the text collapsed to
its ellipsis. The `ui` module documents this defect, names it §59, and says *"there is no way to ask
for the broken combination"*; there is, and §153's tile found it.

The names are now shortened in Rust, by character count from the tile width, and rendered as plain
labels. No truncation machinery, so no layout to get wrong.

*Seventeenth: a component that documents its own trap is not the same as a component that prevents
it — and the trap was two layers away from where the note lives.*


## 165. Four abbreviations you had to hover to find (2026-08-12)

A screenshot of the sidebar beside a screenshot of another app's `⋮` menu, and the ask:

> *"I think we need to improve the app control to create a new project and to create new
> conversations. We can use the vertical 3 dots and when click we can show the options … There is a
> New button so when click new a sub modal menu can say: New conversation and the other option New
> project."*

### Creating a project had no button at all

A project is "a name some conversation is filed under" (§106), so the only route to a new one was
the *file* picker: open a conversation, open the picker, type a name, and the conversation moves
into the folder that naming it created. That is a real gesture and it works, but it is filing —
there was nothing that meant **start** a project, and a researcher beginning a new line of enquiry
starts before they have anything to file.

`New` is now a menu with two rows. `New project…` opens the same picker in a mode where choosing a
name calls `new_thread_in` instead of `file_in_project` — the same list, the same
`New project “…”` first row, the same typing. Only what choosing *does* differs, so there is no
second way to name a project and no second place for the rules to drift.

### One target instead of four, and words instead of characters

Each row carried `rename` and `✕`; each project heading carried `+` and `✕`. All four appeared only
on hover, so the way to discover what a row could do was to point at it and decode two
abbreviations — and `+` beside a heading is not self-evidently "start a conversation in this
project". They are one always-visible `⋮` per row and per heading now, whose contents are
sentences. `New conversation in Late blight` says which project even when the menu is floating over
a list of them.

Nothing new is reachable: every row calls a method the sidebar already had. That is `menu.rs`'s
stated rule for the right-click menu — *"the menu is a second door onto the same room, not a second
implementation"* — and it is why this is a rearrangement rather than a feature.

### §163, one layer further in

A conversation row opens that conversation on click; a project heading opens its folder in
Explorer. The `⋮` sits inside both. Without `stop_propagation`, asking a row what it can do would
*switch conversations* first, and asking a heading would launch a file manager. The same shape as
§163's overlays, at a smaller scale, and reachable the same way: a control drawn on top of a
clickable thing is not thereby the only thing that was clicked.

`menu_card()` is now one function. Both menus need `occlude` and both need the left press swallowed;
the right-click menu had learned both, and a second menu written beside it would have had to learn
them again.

*Eighteenth: an affordance nobody can find is not smaller than a missing one, it is worse — it
looks like the feature exists.*
## 166. The migration could not tell an empty list from a cleared one (2026-08-12)

§154 and §155 shipped, the researcher deleted their test conversations, restarted, and the
conversations were back. Explorer showed `Documents\Mini-Me` holding one file — `subagents.json` —
so the *files* had gone. Only the rows returned.

That is the reverse of §154's failure, which makes it a different bug wearing the same shirt: there
the deletion never became durable, here it did and something put the rows back.

### §90's rescue has no memory

`adopt_untagged_conversations` exists because `dfea94a` began filtering the sidebar on a metadata
tag, which hid every conversation created before it — *"the conversations doesn't load, like this
was erased"* (§90). It searches untagged threads, keeps the ones with human messages, and tags
them. Its own doc says **"Runs once, and only when there is nothing to lose."**

It runs on **every** listing, and its guard is not "has it run" but "is the tagged list empty":

```rust
if !self.list_conversations(1).await?.is_empty() {
    return Ok(0);
}
```

Emptiness is true in two situations the guard cannot tell apart. One is the launch after a pull,
where old history is hidden and the scan is exactly right. The other is **the researcher having
just deleted everything** — and then the scan re-tags whatever threads are left. Background workers'
threads are left, because a conversation's delete removes its own thread and not the ones it
delegated to; the workers carry the task description as a human message, which is the very test
adoption uses to recognise a conversation.

So: delete your last conversation, and the next refresh promotes leftovers into the sidebar. The
folders stay deleted, because that half worked. The rows come back.

### A fact, not a symptom

`adopted_untagged` is now a settings field, written once the scan completes — including when it
adopts nothing, which is the ordinary case and precisely the installation that must stop scanning.
The caller decides and the caller remembers; `list_conversations` takes `adopt` and reports back
whether the scan ran, because "what to show" and "is the migration finished" are two questions and
one return value was answering only the first.

Read from the stored settings rather than from `self.draft`, for the reason `remember_panels`
already gives: the draft is the Settings pane's editing buffer, and this decision has to be made
from what survives to the next launch.

The doc comment was not merely optimistic, it was **false** — "runs once" described a design nobody
had built, and it sat directly above the loop that ran every time. Three sections this week have
turned on a comment asserting something the code did not do (§148's docstring, §161's advice,
§155's guard). The pattern is specific enough to name: when a comment claims a *frequency* or a
*boundary*, that claim is testable, and if it is worth writing down it is worth a test.

*Nineteenth: a one-time migration needs a record that it ran. Inferring it from the state it
produces means it fires again the moment a person legitimately reproduces that state.*


## 167. A project you named, and a sidebar that could not see it (2026-08-12)

§165 gave `New` a menu whose second row is `New project…`. Naming one and pressing Enter left the
sidebar saying *"Conversations you start will appear here."*

> *"When I click to create new project, nothing appears in the conversations panel. This means we
> should create the logic to have empty named projects."*

Correct, and the gap was mine: §165 shipped the affordance and left the state it implies for later
without saying so.

### §106 was right about the registry and wrong about the evidence

*"A project is exactly 'a name some conversation is filed under', so there is no separate registry
to fall out of step with the sidebar."* The first half is the part worth keeping — a list of
projects in the settings file is precisely what §154 deleted, because it survived the conversations
it described and resurrected them. The second half made a project unable to exist before its first
conversation, and `new_thread_in` does not create a thread; it decides where the *next* turn will
write. Until that turn happens there is no thread, no metadata, and nothing for a sidebar reading
conversations to show.

§105 had already settled where the missing fact lives: a project **is** a directory under
`Documents\Mini-Me`. Reading that directory is not a second registry, it is reading the thing
itself — the same argument §106 makes, applied to the evidence §106 overlooked. `workspace::projects()`
lists them and `create_project` makes one, so naming a project creates it and the sidebar shows it
immediately, empty.

### Telling a project from a conversation

Both sit directly under the workspace root. The discriminator is the shape of the name: a thread is
a UUID, a project is whatever a researcher typed. That predicate already existed — §152 wrote it to
strip a leading UUID from an Outputs folder label — so it moved to `workspace` where both callers
can reach it rather than being written a second time.

Files are skipped, which is what keeps `subagents.json` out of the sidebar.

### Two smaller things that fall out of it

`create_project` returns the name the folder **actually** got, and that is what gets stored on the
conversation. `project_folder` rewrites characters a path cannot hold, so keeping the typed text
would file work under `Q1/Q2` while the directory is `Q1_Q2` — and a sidebar reading both sources
would show one project twice, under two spellings.

Empty projects are seeded only when the search box is empty. A filter is a way to find work; an
empty project matches nothing, and leaving them in a filtered list would make searching look broken.

### And then it read them at the wrong moment

Reported on the next launch: the project was not there. The directory listing was written inside
the *answer* to `list_conversations`, so a fact about this machine's disk was waiting on an HTTP
reply — and the first refresh of a cold launch reliably fires before the backend is up, which
`list_conversations` documents in its own comment two lines away. No reply, no headings, and an
empty project simply absent on the launch after it was created.

It is read before the request now, and again on the answer in case a turn created one meanwhile.
The same shape as the bug above it, one layer down: the sidebar had the right evidence and asked
the wrong thing for permission to look at it.

*Twentieth: "derive it, don't store it" is a good rule that says nothing about **which** derivation.
The state existed on disk the whole time; the sidebar was reading the wrong evidence for it — and
then, having found the right evidence, waited on something unrelated before reading it.*
## 168. Setup Stop: the boundary is now concrete, and still not safe blind (2026-08-10)

§146 left this open because a setup repair crosses two process boundaries. Reading the current
ownership makes the failure mode more precise:

1. `Workbench::start_fix` owns only an `UnboundedReceiver<FixEvent>`.
2. `Sidecar::run_fix` owns a Tokio task, which waits on `spawn_blocking`.
3. `preflight::run_streaming` spawns the command, then moves the `Child` into a waiter thread.

There is no cancellation handle at any layer. Dropping the receiver or aborting the Tokio task
does not cancel `spawn_blocking`; its OS process and waiter continue. Adding a Stop button at the
first layer would therefore change only the screen — the exact dishonest result §146 refused.

### The ordinary WSL repair needs a Linux process-group handshake

Every non-runtime fix on Windows is `wsl.exe … bash -lc <script>`. §26 already established that
killing `wsl.exe` does not reliably reap what it launched inside the distro. The safe protocol is:

1. Give each fix a random run id.
2. Inside WSL, start the repair in a new session/process group and publish its **numeric PGID** to
   a run-id-specific control file visible to the Windows app *before* the repair can do work.
3. Stop launches a second `wsl.exe` call and passes that numeric value as a literal argument to
   `kill -- -PGID`: TERM, a bounded wait, then KILL if the group still exists.
4. The UI says **stopped** only after the original command exits and a group-existence probe says
   no process remains. Until then it says **stopping**; a failed probe says it could not confirm.

The numeric handoff matters on this machine. §147 found variables assigned inside `wsl bash -lc`
arriving empty and deleting the checkout when interpolated into paths. A cancellation wrapper that
kept `$pid` or `$pgid` in a generated shell command would place a dynamic value in the same failure
class. Rust must read, validate and write the literal decimal PGID into the separate kill argv.

### An elevated WSL install is a different operation

`Install WSL` / `Install Ubuntu` runs through `Start-Process -Verb RunAs`. ShellExecute owns the
elevation boundary; the unelevated app does not own a killable child tree. The elevated wrapper
would have to publish its PID, and Stop would need a **second UAC-approved elevated** `taskkill /T`
request. Refusing that second prompt means *not stopped*. Closing the visible elevated console may
also be graceful rather than terminal during a Windows component install, so the app cannot infer
success from the window disappearing.

That policy needs a disposable Windows VM: force-killing `wsl --install` on a developer's real
machine to see whether Windows servicing remains recoverable is not an acceptable test. The UI
must say in advance that stopping an OS install asks for admin rights again and may still require a
restart; it must never reuse the ordinary repair's one-click wording.

### The tests that make implementation safe

- **WSL tree test:** a wrapper starts a shell, child and grandchild in one new group; all three
  append heartbeats. Cancel by the published PGID, wait, and prove every heartbeat stops and the
  group-existence probe fails. Run this on the target Windows/WSL pair, not native Linux alone.
- **Race test:** cancel before the PGID control file exists, while it is being written, and after
  the process exits naturally. No empty/unvalidated value may ever reach `kill`.
- **UAC refusal test:** refuse the second elevation prompt. The pane must remain *not confirmed
  stopped*, retain the log, and allow another attempt.
- **Disposable-VM servicing test:** cancel both `wsl --install` and distro installation, reboot,
  and prove Setup can run the same repair to completion afterwards.
- **Window-close test:** close the app during each kind of repair. The same cancellation policy
  must run, or the app must explicitly leave the independently elevated install visible; silently
  detaching is not a third policy.

This environment cannot run the first test: `wsl.exe --status` returns Spanish
`Acceso denegado`, `Wsl/EnumerateDistros/Service/E_ACCESSDENIED`. It also cannot safely perform the
disposable-VM test. No code or cosmetic Stop control is added in this task; the implementation is
blocked on those two target-platform proofs, not on an unknown design.

*A button is not cancellation. The proof is that the grandchildren stopped.*


## 169. A rail whose line stopped a third of the way down (2026-08-12)

The road shipped with §74 and has looked wrong ever since without anyone naming it. Put beside the
design it was drawn from, two faults:

> *"We need to fix how we connect the dot for the road. And its awful when closed."*

### `items_start` is why the line never arrived

Each stage is a row: a fixed-width gutter holding the dot and, below it, a connector that continues
to the next stage. The connector is `flex_grow` with a 14px minimum, which is the right shape — it
should take whatever height the row turns out to be.

The row was `items_start`. That aligns children to the top *and leaves them at their content
height*, so the gutter stood 23px tall — a 9px dot plus the connector's minimum — while the row
beside it stood at 46px for a two-line label. The connector had no height to grow into and stopped
a third of the way down, so every dot hung unconnected under a stub.

Removing `items_start` lets the gutter stretch to the row, and the connector then spans it. The
label column takes `items_start` for itself, which is what it actually wanted; the row's copy was
doing that job by accident and breaking the rail as a side effect.

### Folded, the rail *is* the content

At 38px there are no labels, but the rows still reserved 46px each — so the dots sat far apart with
stubs between them, which is the "awful when closed" state. Folded rows are 26px now, which closes
them into one strung line. The same numbers, named: `ROW_OPEN` and `ROW_FOLDED`.

### And the third toggle finally has an icon

§157 gave Settings, the conversation list and the research panel drawn icons and left the road
reading `◧ road` beside them. `road.svg` is the rail itself — a line, two filled dots and an open
one — which is the same picture the strip draws, at 14px.

### Shipped as a claim before it was shipped as a change

§169 went in describing the fix above, and the fix was not in the commit. The script making the
edit asserted on its third replacement, raised, and exited **before writing the file** — so the
first two edits, the ones that mattered, were discarded. `cargo build` and 266 tests passed,
because an unchanged file compiles perfectly. The icon in the same commit *did* land, which made
the result look like a partial success rather than a no-op.

Two things follow. A batch edit that fails partway must not leave the earlier work unwritten and
the reader believing otherwise — write each change or none, and check the file afterwards rather
than trusting the tool that reported success. And a green suite is not evidence a change landed:
these tests never touched `road_strip`, so they were as green before the edit as after.

The researcher found it the only way it could be found, by looking at the window: the dots were
still hanging under stubs, in a build whose plan said they were not.
### Folded, it sat against the left edge

The gutter is the row's only child when folded, and the row is `w_full`, so a 12px rail sat at
x=0 of a 38px strip. `justify_center` when folded puts it down the middle, which is where the
design the researcher supplied has it.

The chevron above it had the same fault for a different reason and was missed on the first pass:
the header is `justify_between`, which puts a **lone** child at the start, and folded the chevron
is the only child. So the fold control sat against the left edge above a rail that had just been
centred — which is more obviously wrong than either was alone. Same one-line fix.

*Twenty-first: a `flex_grow` child is a promise about a parent that has a size. `items_start` takes
that size away, and the symptom appears two elements away from the line that caused it.*

*Twenty-second, and the more expensive one: "the tests pass" answers a question about the code that
is there. It says nothing about whether the code you wrote is the code that is there.*

## 170. The handshake §168 specified would have caused the orphaning it prevents (2026-08-12)

§168 was written by reading code and reasoning from §26. It was then **run**, on the target
Windows/WSL pair, by the second agent. Two of its three claims held and the recommendation did not.

Measured:

- **Process-group cancellation works.** PGID `450` published, `kill -- -450` stopped the shell,
  both children and their `sleep` processes; heartbeats `126 → 126`.
- **The races behave as §168 assumed.** Read immediately, the control file is absent; a second
  later it holds a complete `445\n` with no partial write seen. A repeat kill answers "No such
  process", which a caller must read as *already stopped*. Blank and non-numeric values still have
  to be rejected before the argv is built.
- **Killing the attached `wsl.exe` reaps everything.** Wrapper PID `28084`; after `Stop-Process`
  the wrapper exited and every Linux descendant went with it. Heartbeats `3 → 3`.

And the finding that overturns the design:

> *Launching through `setsid` caused `wsl.exe` to exit by itself while the Linux process tree
> continued running.*

`setsid` is what detaches the repair from the very Windows process the app already owns and can
already kill. §168's protocol would therefore **create** the orphan it was designed to prevent —
the app would hold a handle to a wrapper that had already exited, and a tree that no longer
answered to it.

### What §26 actually established

§26 is why §168 reached for process groups at all: *killing `wsl.exe` does not reliably reap what
it launched inside the distro*. On this machine, today, it does. Either the platform changed or §26
described a detached case and the note generalised it. Nothing here settles which, and the honest
record is that the claim was carried forward for months without a measurement.

### The implementation this leaves

Keep the ordinary repair attached to the `wsl.exe` the app spawned; hold that `Child`; make Stop
terminate it and treat "already exited" as stopped. `preflight::run_streaming` currently moves the
`Child` into a waiter thread with no handle escaping — that, not the shell protocol, is the change
to make. The validated PGID mechanism stays documented as the fallback for a future repair that
deliberately detaches.

Elevated installs are untouched and still separate: §168's second-UAC policy stands, and the
disposable-VM test was correctly not run on the researcher's machine.

*Twenty-third: a specification derived from reading is a hypothesis. This one was careful, cited its
evidence, and was wrong in the direction that would have hurt — the mechanism it added was the
mechanism that broke the thing already working.*


## 171. Twelve drawings, and not one logo (2026-08-12)

The deferred item from §164, asked for plainly: *"if its a python script the symbol of python must
appear. its the same for json, etc etc etc."* Every file tile drew one of four geometric glyphs, so
a `.py`, a `.json`, a `.log` and a `.txt` were the same mark in the same colour, and the name did
all the work.

The suggestion was to take Zed's set. Two reasons not to. Zed is GPL-3.0 and its `assets/icons` are
a mix of original and Lucide-derived work, so copying the directory means auditing provenance
file by file and carrying attribution — against the rule this organisation sets about third-party
material. And they would not match: the five icons §157 shipped share a 24px canvas, a 1.55 stroke
and round caps, and a borrowed set beside them looks worse than the glyphs did.

So they are hand-authored on the same canvas, and they are **format families rather than brand
logos** — the second half deliberate. A recognisable Python or Docker mark is a trademark, and
reproducing one inside a shipped product is a different question from drawing a file with angle
brackets on it. `.py`, `.r`, `.jl`, `.sh`, `.sql` share one *code* icon and differ by nothing;
what tells them apart is the filename, which is already on the tile. What the icon settles is the
question a person actually has scanning a folder: is this a table, a picture, a script, a
notebook, a config, a document, a log, an archive, a database.

Twelve, sharing one page-with-a-folded-corner outline so a column of them lines up, with the
distinguishing mark in the lower two-thirds.

The test asserts the thing that can silently break: **every mark `file_mark` can return is a
declared and loadable asset**. A new extension arm naming an icon nobody added would draw nothing
at all — §157's failure one layer along — and a count would only have reported that the number
changed.

*Twenty-fourth: "use their icons" is a licence decision wearing the clothes of a shortcut.*


## 172. Stop, built from what the measurement left standing (2026-08-12)

§28 asked for it, §146 refused it, §168 specified the wrong mechanism and §170 measured that. What
remains is small.

`preflight::Cancel` holds a pid and nothing else. `run_streaming` arms it the moment `spawn`
returns — before a line is read, so a Stop pressed during a cold WSL start has something to act on,
which is the race §168 wanted a control file to solve — and disarms it once the child is reaped.
While armed the waiter thread still holds the `Child`, so neither operating system can hand that
number to anyone else; the late-click hazard is closed by construction rather than by a check.

The button says **stopping**, not stopped. The only honest report of a stop is the command
exiting, and that arrives as `FixEvent::Finished` on the same channel as any other ending. A repair
that finished between the click and the call is *stopped*: `Cancel::stop` reports there was nothing
to signal, and nothing left to stop is the outcome the button was pressed for.

### The test found what the reasoning had not

The first Unix implementation signalled the child alone. The test hung, and the assertion that
fired was the one about elapsed time.

`sh -c "…; sleep 30; …"` killed at the shell leaves `sleep` running **and holding the inherited
stdout pipe open**, so `run_streaming` blocks on EOF until the grandchild finishes by itself — a
Stop that reports nothing for thirty seconds. That is §26's complaint reproduced in miniature, and
it is the thing §168 built its whole protocol around.

So the child is spawned into its own process group on Unix and signalled with a negative pid.
Windows keeps the attached wrapper, because there `wsl.exe` *is* the group leader in every sense
that matters and §170 measured that killing it takes the Linux tree with it. The asymmetry is the
measurement, not an oversight: the same gesture that fixes Unix — `setsid` — is what breaks
Windows.

Which means §168 was not wrong about mechanism so much as wrong about *where*. Process groups were
the answer, on the platform nobody was worried about.

### Still not done

Elevated installs. `Start-Process -Verb RunAs` puts the child outside this app's token, so Stop
cannot reach it and the button must not pretend otherwise — §168's second-UAC policy stands
unimplemented and the disposable-VM test remains unrun. The button appears for ordinary repairs;
an elevated one needs its own wording before it gets one.

*Twenty-fifth: the measurement that overturns a design does not always overturn its mechanism. This
one moved it to the other platform.*


## 173. Four complaints about chrome, and one about ownership (2026-08-12)

A screenshot of a finished EDA and a short list. Three are the same kind of thing; the fourth is
not.

### The road was inside the conversation's card

> *"The conversation panel (center panel) is colliding with the road."*

The sidebar is a card. The research panel is a card. The road was a bordered strip **inside** the
chat pane's card, so the two read as one panel with a notch cut out of the left of it, and folding
the road left a 38px stub of a different background attached to the transcript. It is its own card
now, a sibling of its neighbours in the root row, with the same `m_1 rounded_lg border`.

That was §74's arrangement and it made sense when the road was decoration inside the conversation.
It stopped making sense when the road gained a fold control, a full-graph button and its own
identity.

### A scrollbar is a control, and a control that is always drawn is furniture

Both — the transcript's and the research panel's — sat permanently against the right edge, close
enough that a long line of the answer ran under the transcript's. They are revealed on hover of the
region they scroll, which keeps them findable exactly when someone reaches for one: the pointer is
already there. `SCROLL_GROUP` names the region, and the two helpers that paint a thumb both watch
it, so a third scrollbar cannot be added that forgets.

§40 asked for a *visible* affordance because there had been none at all. Visible on approach still
satisfies that; permanently painted was one reading too many.

### `memories/` was never the researcher's

The Outputs panel listed `memories\instructions.txt`, 436 B. That is where the agent keeps its own
instructions between turns. A panel headed **FILES** invites a person to open, rename or delete
what it lists — and deleting that one silently changes how the agent behaves afterwards, with
nothing to connect the two events.

It joins dotfiles and `__pycache__` in the skip list. The line those three share is not "hidden" or
"cache", it is **not the researcher's to manage**, and the comment now says so.

*Twenty-sixth: "show everything" is a defensible default for a file list right up to the moment the
list contains something the reader can break.*


## 174. Half a padding, silently (2026-08-12)

> *"The conversation is too close to the left end. That's a visual problem."*

§156 replaced the eager scrolling `div` with `gpui::list`, and carried its `.p_4()` across. On a
`div` that is sixteen pixels on four sides. On a `list` it is sixteen pixels on **two**:

```rust
// gpui-0.2.2/src/elements/list.rs:812
let mut item_origin = bounds.origin + Point::new(px(0.), padding.top);
```

The horizontal half is computed from the style two lines earlier and then never used. So the
transcript moved flush against its own border, and the change that did it looks like a
straight port.

Nothing could have caught this. It compiles, every test passes, and the property is "sixteen
pixels of nothing on the left" — visible in a screenshot and in no other artefact. It arrived with
§156 and was reported after §173 moved the road out of the pane, which is what removed the
accidental inset that had been hiding it.

The inset is on the row now, where a `div` honours it, and named: `TRANSCRIPT_INSET`, matching the
composer below so a question and its answer start on the same x.

*Twenty-seventh: a property that a container silently drops is worse than one it rejects. `p_4`
should not compile on an element that means `py_4`.*
## 175. The claim was never checked against the folder (2026-08-12)

The oldest red item, and the one that mattered most: an answer would list ten filenames and the
Outputs panel would show none of them. Twice, a turn reported plots saved after the command that
would have written them had failed. The system prompt already says *"NEVER invent findings,
numbers, or charts"* — the third capital-letter rule this project has measured at zero compliance,
which is what a rule with nothing behind it is worth.

The app had the answer the whole time. `collect_plots` walks the conversation's folder at the end
of every turn to find files no message has claimed yet; the same walk says what the folder holds.
Nothing compared it to what the answer said.

### Reading a name out of prose without inventing one

`named_files` is the whole risk. A false positive puts a correction under a sentence that was fine,
and a warning that cries wolf is one nobody reads on the day it is right. So:

- The extension must be one a research run actually **writes**. `CLAIMABLE` is deliberately
  narrower than `file_mark`'s list — no `.sh`, `.js`, `.rs`, because an answer is likelier to
  mention one in passing than to have produced it.
- The stem must contain a letter and be at least two characters. `0.96` fails on the extension;
  `figure 4.png` fails here, while `fig4.png` passes.
- Punctuation is trimmed, backticks and asterisks are separators, and a path is reduced to its
  basename — the question is whether the file exists, not whether the model recited its directory.

Tested against the real answer that prompted this and against seven lines of prose that look like
filenames and are not, including `annual_income vs monthly_spend = 0.96` and a `doi.org` URL.

### Two things it deliberately does not do

**It does not accuse.** A named file can be absent because the command failed, because it landed
outside the workspace (§160), or because the answer invented it. The app cannot tell those apart,
so it reports the check — *"named above but not in this conversation's folder"* — and no verdict.

**It does not fix the answer.** Editing the model's text would make the transcript disagree with
what was actually said, which is a different lie and a worse one.

### Why it self-corrects

Recomputed over every assistant message each time outputs settle, not fixed when the turn ends. A
background worker can still be writing, so a name missing at second one and present at second three
was never a false claim — and flagging it would be its own kind of dishonesty. The workspace only
grows, so the note disappears on its own when the file arrives. Names the *researcher* introduced
are excluded outright: dropped files are read where they lie (§13), and an input on the desktop was
never supposed to be in the folder.

*Twenty-eighth: the prompt had the rule and the app had the evidence, and for four months neither
one knew about the other.*


## 176. `/ok` answered before the graph existed (2026-08-13)

Reported as two delays: saved conversation names taking time to appear, and a saved conversation
taking time to open. The initial suspicion was DeepAgents plus SQLite. The backend log separates
them conclusively:

- the LangGraph HTTP server started in **1.914–2.121 seconds**;
- the custom `AsyncSqliteSaver` loaded in **0.00062 seconds**;
- `POST /threads/search` returned two titles in **290 ms** in the direct Windows probe; and
- the first `GET /threads/{id}/state` spent **14,154–14,982 ms** loading `agent`.

That last request was read-only, but its log showed network handshakes with Asta, Dataverse,
AGROVOC and Crop Ontology. `backend/agent.py:130-133` awaits those MCP tool bundles while assembling
the DeepAgents graph, so reading stored state paid for every research tool before it could read the
conversation. SQLite was not the bottleneck.

### Expensive once per process, inexpensive on later graph access

The MCP tool registry is cached at process scope. The same real backend process reported later
graph factory accesses at **356.13 ms** and **249.4 ms**, with no repeated MCP handshakes. So the
14-second part is a cold-process cost landing on the wrong interaction, not a 14-second tax on every
conversation. The remaining quarter-second factory work is real and is the reason lazy graph
construction is still the eventual backend fix, but it is not the defect the researcher felt.

### The health check claimed a boundary the server does not provide

`LangGraphClient::is_healthy` documented `/ok` as meaning *the server is up and the graph is
loaded*. The log had `/ok` answering before any factory call, then `Slow graph load` on the first
state request. The comment now says what the endpoint proves: HTTP readiness only.

Three candidate warm-up routes were exercised against the installed LangGraph API 0.9.0:

- `GET /assistants/agent/schemas` returns 422 because this version requires a UUID there.
- `POST /assistants/search {"limit":1}` returns without loading the graph.
- Creating one assistant with `graph_id: "agent"`, then requesting its `/schemas`, reaches the
  graph factory. Repeating the create with a fixed UUID plus `if_exists: "do_nothing"` returned
  200 both times, so launches converge on one internal record rather than accumulating them.

The diagnostic shell deliberately did not extract the app's keychain secrets, so its schema probe
stopped at the expected `ASTA_API_KEY` check. The earlier real app log supplies the successful,
credentialed timing and the complete MCP sequence.

### Warm the graph without making the sidebar wait for it

Graph warm-up stays separate from backend warm-up. As soon as `/ok` answers, the UI refreshes the
conversation list and project spine, then says **loading research tools…** while the fixed internal
assistant's schema request constructs the graph. A 60-second request budget matches the existing
backend health budget: hotel Wi-Fi may make one MCP host unreachable, but it cannot leave the
desktop claiming to start forever. Failure is logged and the status says the tools are not ready;
it does not invent success.

This intentionally does not add a sidebar cache. §154 and §166 already show what happens when the
client remembers conversation facts after the backend has deleted or reclassified them, and the
measured warm `/threads/search` is not slow enough to justify a second registry.

The first process still spends roughly 15 seconds connecting to four MCP servers sequentially; the
client change moves that cost off the first click, it does not erase it. Gathering those clients
concurrently, and ultimately keeping read-only state routes out of graph construction entirely,
belong in the protected Python backend and remain the next performance work.

*Twenty-eighth: readiness is not one fact. A socket can answer, history can be listed, and the agent
can still be fifteen seconds away from usable.*


## 177. The app had a spinner and never showed it when it mattered (2026-08-13)

> *"The delay at loading the conversations doesn't show any animation that says loading."*

There **is** a moving mark. It has been in the status bar since §51, four braille frames on a
repeating animation, and the comment beside it says exactly why:

> *"a still window during that reads as a hang, which is the single most common reason someone
> kills an app that was working fine."*

It is shown `when(self.streaming || self.running_fix.is_some())`. So it appears while a turn
streams — when the transcript is already filling with tokens and nobody could mistake the app for
stuck — and stays hidden through the two longest silences in the product: the fifteen seconds of
graph construction at launch (§176), and the pause while a conversation loads. The right reasoning,
attached to the wrong condition.

### One question instead of two booleans at a call site

`is_waiting()` gathers all five: a streaming turn, a running setup fix, the graph warming, a
conversation opening, and the conversation list not yet loaded. The status bar asks that rather
than naming states, so the next wait somebody adds is covered by adding it to one predicate instead
of being forgotten in a `when`.

Two of those needed a flag. `opening` and `warming` exist because the app knew about both only as
*prose* — `"opening…"`, `"loading research tools…"` — and prose cannot be asked a question. That is
§79's rule again: matching on a message to discover a fact is how the two get out of step.

### And a second one, where the researcher is looking

The status bar is at the bottom of the window; the wait being complained about is in the sidebar at
the top left, under a heading that already said `Loading your conversations…` and said it
motionlessly. There is a mark beside it now, muted rather than accent — this reports a state, and
in this app the accent means *act on me*.

`ui::Spinner` is a component for the reason every other one in that module is: the version that
existed was fifteen lines inline at one call site, so the second place that needed it got a
sentence instead.

*Twenty-ninth: a feature that exists and is wired to the wrong condition is harder to find than one
that was never built, because the code review that would catch it sees the feature and stops.*


## 178. An invitation to start, over a conversation already chosen (2026-08-13)

> *"When I click a conversation and it is opening, I would like to see it at the middle panel
> rather than at the bottom left."*

`open_conversation` clears the transcript before its fetch lands, so for the width of that request
`self.transcript.is_empty()` is true and the centre drew the empty state — *"What are you working
on?"*, with three suggestions for starting something. Offered over a conversation the researcher
had just chosen, because the app had nothing else to draw there.

§177 put the honest report in the status bar, which is the right place for a *second* copy and the
wrong place for the only one: the bottom-left corner is the furthest point in the window from the
row that was clicked and from the space the answer is about to fill.

The centre now says which wait it is, in the place the result will appear.

Deliberately plain — a mark and a sentence. The obvious upgrade is a skeleton of grey bars, and
that is the one to be careful with: it has to guess how many messages are coming and how tall each
is, and a wrong guess makes the real transcript jump when it arrives. Recorded as an open item
rather than improvised here, because "cool" and "does not lie about what is coming" are two
requirements and only one of them is free.

*Thirtieth: an empty state is an answer to "there is nothing here", not to "I do not have it yet".
The app had one string for both, so it gave the confident answer to the uncertain question.*

## 179. The feature nobody could find, and the question it ate (2026-08-13)

§28 built "local file → analysis" — the MVP's *one thing the web app cannot do* — and then
closed with a line that stayed true for twelve days: **"never dropped anything."** Reading it
before verifying it found two defects that a live drop would have shown in about four seconds,
and one that it would not have shown at all.

### It overwrote the question

`files_dropped` called `composer.set_text(prompt)` unconditionally. So the sequence a person
actually performs — decide what to ask, type it, *then* go and fetch the file — destroyed the
first step at the last one. Silently: the composer simply held different text than the one they
had written, with nothing to say it had been replaced.

The composer now keeps what is there and puts the paths underneath it, after one blank line. It
adds no prose of its own, because a prepared sentence appended to somebody's question reads as a
second question arguing with the first about what to do with the file. *(This first kept the
prepared sentence for the empty composer. §180 removed that too, on sight.)*

### It was invisible

Dropping was announced in one line of the **empty state** — a screen that vanishes the moment a
conversation has anything in it. From the second turn onwards, nothing in the window said the
app took files at all.

And dragging is the harder gesture on the platform this app is for. It needs Explorer and a
*not*-maximised window arranged side by side, which is not how anybody works; a researcher who
has just found their CSV is looking at a full-screen file manager. So the composer now carries a
clip, left of the field, opening the platform's own chooser. Files only — Windows'
`can_select_mixed_files_and_dirs` is `false` because `FOS_PICKFOLDERS` *toggles* the dialog
rather than widening it, so asking for both would hand a folder picker to someone looking for a
spreadsheet. Dragging still accepts a folder, which is the gesture that suits one.

Dragging also now shows that it will be accepted: the composer lights up while a file is over
the window. It is styled through gpui's `drag_over` rather than a flag on `Workbench`, and that
is not a preference — `FileDropEvent::Exited` clears `active_drag` but is dispatched to no
element, so a flag set on enter has no way to learn the drag left and would stay lit until the
next drop.

### The one a live drop would not have caught

`wsl_path` translates `C:\…` to `/mnt/c/…` by reading the drive letter. A **UNC path** has none,
so it falls through to the pass-through arm and the agent receives `//nas/shared/yield.csv` — a
path that exists in no Linux filesystem. The turn then fails a minute later with
`FileNotFoundError`, naming neither the share nor the reason.

This would not have shown up in a test drop from the Desktop, and it is not a rare shape here: a
CGIAR centre keeps data on network shares. `can_open` is now asked **before** anything reaches
the composer, and the refusal names the file and the fix a non-programmer can act on — copy it
to this computer first. Mapping the share inside the distro is the other fix and is not one to
suggest to someone who does not code. Files that *are* reachable still go in; the ones left out
are named rather than counted, because "3 of 4 added" leaves them hunting for which.

### What is still unverified

The same thing as before, and it is the user's step: no file has been dragged onto a real
window, and no chooser has been opened on Windows. What has changed is that the code no longer
has three defects waiting behind that gesture.

*Thirty-first: "built" and "used once" are different claims, and a plan that records the first
in the tense of the second will keep its defects for as long as nobody tries it.*

## 180. The app was writing the research question (2026-08-13)

> *"When we attach a file I see a prefilled text. Let's avoid that so the user can have
> flexibility in his query."*

§28 filled the composer with a whole question — *"Analyse the data in
`/mnt/c/…/yield.csv`. Start by describing what it contains."* — and §179 kept it for the case
where nothing had been typed yet, on the argument that it was the case §28 was written for and
the one where a prepared sentence is genuinely useful.

Seen once, that argument does not survive. **The prepared question is a guess about the
research, written by the only participant who has not seen the data.** "Start by describing what
it contains" is a reasonable opening for a stranger's CSV and quite wrong for a scientist who
knows exactly what is in theirs and wants the second question, not the first. The cost of
guessing is not that the sentence is unhelpful — it is that it has to be *deleted* before the
real question can be typed, which is work the app created.

What is left is the part the app actually knows: **where the file is**. That is genuinely worth
having, because `/mnt/c/Users/…/2024_yield_trials_huancayo.csv` is not something anyone should
retype, and getting the WSL spelling right is the one piece of this a researcher could not do
themselves.

So the composer gets the path and nothing else:

- **Nothing typed yet** — the path, then a blank line to write under. The blank line is
  load-bearing: `set_text` leaves the caret at the end, and without it the first character typed
  joins onto the filename.
- **Something typed** — the path underneath it, after a blank line, every word left alone.

The status bar takes over the job the prefilled sentence was doing badly — *"added yield.csv —
say what you want done with it"* — where it costs nothing and deletes nothing.

Two things fell out. `prompt_for_dropped` is gone entirely, and with it the directory special
case: *"Have a look at the files in …"* existed only to write a sentence, and with no sentence to
write a folder is a path like any other. The `directories: &[bool]` parameter and the `is_dir()`
call that fed it went with it — a syscall per dropped file, spent on prose.

*Thirty-second: a prefilled field is a guess with the confidence of a decision. Prefill what
cannot be got wrong — a path, a name, a number the app measured — and leave the sentence with an
opinion in it to the person who has one.*

## 181. A potato centre has more colours than its logo (2026-08-13)

> *"Usually they use orange and brown but these colours are painful to see together. Because we
> are the potato center, potatoes have a variety of beautiful colours when we talk about native
> potatoes."*

The sources agree with both halves of that observation. CIP's official branding guide names
**`#EE7203` orange and `#5D2E00` brown** as the primary pair, then gives green, red, purple,
magenta, yellow and cyan as its secondary publication palette. CIP's own native-potato material
is broader and more specific to the institution's subject: more than 4,000 Andean varieties,
with flesh and skins running through white, yellow, pink, red, purple, blue and black
([brand guide](https://cipotato.org/site/logo/pdf/BrandingGuidelines.PDF),
[native varieties](https://cipotato.org/potato/native-potato-varieties/),
[potato nutrition](https://cipotato.org/potato/potatonutrition/)). The logo pair is therefore a
rule for the logo, not an obligation to make the whole workbench orange-on-brown.

Zed's [Theme Builder](https://zed.dev/theme-builder) made the other boundary explicit. It does
not offer one global colour replacement: it separates eleven surface roles, six borders, six
text roles, editor/navigation/element states and the status colours, with values able to link to
one another. This app has fewer roles, but the same semantic split. A good palette here therefore
needs a quiet surface ladder first and crop colours assigned to jobs; distributing saturated
potato colours evenly across the window would be a poster, not a reading tool.

### Papa Nativa

The new built-in and fresh-install default is **Papa Nativa**:

- aubergine-black surfaces, rising from `#18141C` to `#352A3C`, take the dark-purple and black
  end of native-potato diversity without tinting every line of text;
- cream text carries long reports; muted mauves keep timestamps and labels in the same family;
- pink-purple is reserved for interactive controls, preserving the rule from §49/§118 that an
  accent means *you can act here* rather than *this heading is important*;
- green, gold, coral and highland blue remain separate success, warning, error and running
  signals. They are the small places where the crop's variety is useful rather than noisy;
- CIP orange survives only in `accent_soft`, as **12% orange over the surface**. `Theme` stores
  opaque `u32` fills and the Zed importer deliberately drops alpha, so the blend is
  pre-composited as `#3A2522`. It looks like low-alpha orange on every panel instead of changing
  with whatever happens to be behind it.

The existing six themes stay available; a saved preference also stays saved. Papa Nativa becomes
the default only where no preference exists, because changing someone's chosen palette during an
upgrade would turn a design improvement into a settings bug. The existing contrast and elevation
tests run over the seventh palette unchanged: every ink/surface pair clears WCAG AA and every
surface step still rises in luminance.

### Installing without removing was only half a gallery

The gallery wrote Zed JSON files into `themes/` and then forgot where each displayed palette came
from. That was enough to install but not to uninstall, and a palette name could not recover the
answer: one Zed file may contain a family of several names, while one file may also override a
built-in.

`ThemeEntry` now carries the exact source file alongside the name and palette. Built-ins have no
source and no **remove** control. A palette read from disk does; removing it removes that one JSON
file and therefore every palette the family contains. The tooltip states that scope before the
click. If the file overrode a built-in, the bundled palette underneath is immediately reapplied.
If it supplied a name that no longer exists, the live choice and Settings draft both fall back to
Papa Nativa — no dead name is left to fail silently at the next launch.

Deletion accepts only an immediate `.json` child of the app's own `themes/` directory. That check
is repeated at the filesystem boundary even though the UI obtained the path from the loader: a
stale callback should not be able to turn *remove this theme* into removal of settings or research
data. `uninstalling_one_zed_file_removes_its_whole_family_and_nothing_beside_it` proves both sides
with a two-palette family and a neighbouring file.

### What the Windows validation found

The first full run was 276/277: the UNC-drop test constructed its supposed host backend with
`BackendConfig::default()`, which deliberately selects WSL on Windows. The assertion passed on
Linux and failed on the target platform because its fixture changed meaning with the OS. The test
now sets `wsl: None` explicitly; application behaviour is unchanged. The second run is **277/277**
and `cargo clippy -p mini-me-desktop-app --all-targets` is clean.

The remaining check is visual and belongs on the real window: open **Settings → Theme**, choose
**Papa Nativa**, and inspect a long answer, a selected conversation, an approval card and a
running specialist. The correct result is cream text on purple-black panels, pink-purple only on
actions, and orange visible as a quiet selected-row tint — never orange text on a brown panel.
Install any Zed theme, reopen the theme list, press **remove**, and confirm every palette from that
installed family disappears immediately while Papa Nativa and the other built-ins cannot be
removed.

*Thirty-third: brand fidelity is not counting how many times a logo colour appears. It is keeping
the logo exact, then using the institution's real subject matter to give the product a colour
language suited to the work.*

### §181 addendum — removing a theme is not a draft (2026-08-13, review)

Two corrections found reviewing the branch before merge. The palette itself needed none: the
contrast and elevation tests enumerate every ink against every surface, and Papa Nativa clears
them, so *"cream stays readable on aubergine"* is machine-checked rather than asserted.

**The removal did not survive Esc.** `uninstall_theme` fixed `applied_theme` and `draft.theme`
and stopped there, and everything else in that pane is deliberately a draft — the dismiss path
reloads `settings.toml` because *"an unsaved palette was a look, not a change"*. Deleting a file
is not a look. So: remove the theme you are using, press Escape, and the app restored the name of
a palette whose JSON no longer existed. `apply_theme` fell back to the default, so the window was
painted correctly while the dropdown read `Catppuccin Mocha` — and because the name was still on
disk, no restart cleared it. §181 claimed the opposite in prose (*"no dead name is left to fail
silently at the next launch"*), which is the exact shape this document keeps recording: the
sentence was true of the two places the code touched and false of the third.

The stored name is now rewritten at the moment of removal, through a fresh `Settings::load()`
rather than by saving the draft — the draft may be holding model or key edits nobody chose to
keep. It is asked separately from the live choice because those two strings drift apart as soon
as somebody previews a theme, and `theme_after_removal` exists so the rule can be tested against
both.

**A comment outlived its decision.** `BENCH` still opened *"Neutral paper and one deep teal. The
default"*, and it is no longer the default. Worth more than the one-word fix, though, is what the
paragraph under it said and this change did not answer:

> *A light default is a deliberate reversal. The app opened on charcoal because editors do, and
> this is not an editor — it is read next to a bench, a greenhouse window and a projector, and
> those are the rooms a dark UI actually fails in.*

That argument is about **rooms**, and §181's is about **colour identity**; the second does not
refute the first, it changes the subject. Papa Nativa is the better palette on the merits and it
is dark, so a fresh install now opens dark in a building full of greenhouses. Recorded rather than
reverted, because the answer if it bites is a light Papa Nativa, not a return to teal — and only
fresh installs are affected, so nobody's saved choice moves.

*Thirty-fourth: when a decision is replaced, check what the old one argued, not just what it did.
An argument about the room is not answered by an argument about the palette.*

## 182. Cream is not an off-white (2026-08-13)

> *"In my eyes the letters and the background compete so it disturbs the attention. Also we
> should have a dark and a light theme for Papa Nativa."*

Measuring the shipped palette turned that into two numbers, and they say the same thing:

| | hue | saturation |
|---|---|---|
| `text` `#f2ebdd` | 40° (yellow) | **44.7%** |
| `background` `#18141c` | 270° (violet) | 16.7% |

The ink was not an off-white. It was **a colour**, at nearly half saturation, sitting 130° across
the wheel from the ground it was set on — and carried at 15.3:1 luminance contrast, which is
almost the maximum available and well past AAA's 7:1. Opposed chroma at near-maximum contrast is
precisely the arrangement that vibrates. The report was not a matter of taste; it was a
description of what those numbers do.

Nothing in the suite could have caught it. `every_shipped_theme_is_readable` measures contrast,
and cream on aubergine passes contrast *magnificently* — 15.3:1 is the problem, not the evidence
of its absence.

### The fix is two numbers

The ink moved into the background's own hue family — 283°, under 10% saturation — and the
contrast came down to 12.8:1. Still far above AAA, without the glare. **Hue belongs to surfaces
and accents; text is not where a palette should be expressed.** The aubergine ladder is untouched,
so the identity survives intact; what changed is that the letters stopped arguing with it.

`the_ink_a_whole_answer_is_set_in_stays_near_grey` now pins it, on **channel spread** rather than
HSL saturation — that measure explodes near white (it rates the cream 45% when its channels span
21 of 255), so a threshold written against it would have to differ between light and dark themes
to mean the same thing. The cap is 16: the highest body text among the eight shipped palettes is
13, and the cream that caused the report was 21. Verified by reintroducing the cream and watching
it fail.

It applies to `text` alone. `text_muted` and `text_faint` are timestamps, labels and counts —
small, sparse, and a tint there is part of how they read as a lesser role rather than as dimmer
body copy. Slate's faint ink spans 23 and is right to.

### The brand palette, measured rather than adapted

The secondary CIP palette was supplied as swatches. Measuring all fourteen against both grounds
found that most of them need no adjustment at all:

- **Dark, shipping exactly as the brand guide prints them:** 369 C `#76B82A` (6.98:1) as
  `success`, 137 C `#FBBA00` (9.78:1) as `warning`, Process Cyan `#009FE3` (5.70:1) as `running`.
- **Light, likewise:** 2607 C `#56217A` (10.50:1) as `accent`, 364 C `#34752D` (5.33:1) as
  `success`.

Four needed moving, all of them for the same reason — a colour bright enough to print cannot also
carry 4.5:1 against paper, and a print colour dark enough to sit on white cannot carry it on
near-black. Only lightness and saturation moved; every hue is **within 3° of its source**, and
three of the four are within 1°. That is the same rule §(Bench) already applied to its two adjusted
inks, and it is what keeps "we use CIP's palette" a true sentence rather than a gesture.

### Papa Nativa Light

The counterpart §181's addendum said to build, and the reason it was worth predicting: the
argument for a light default was never about colour identity, so a darker identity did not answer
it. *"It is read next to a bench, a greenhouse window and a projector, and those are the rooms a
dark UI actually fails in."* Now there is a light Papa Nativa, on the same 270–280° hue family, so
the two read as one identity under two lights rather than as two themes.

**The default is unchanged** — a fresh install still opens on the dark one. Moving it is a
decision about where these researchers actually sit, and making that call quietly while shipping a
palette is exactly what §181's addendum objected to.

*Thirty-fifth: a passing contrast test says the text can be read, not that it is comfortable to
read. 15:1 and 4.4:1 fail in opposite directions and only one of them has an assertion.*

## 183. Measuring a reading tool with an editor's instrument (2026-08-13)

> *"I still don't like these. Light is better, but remember: the users are not coders. It seems
> these themes are for coders. Our users are scientists that read, analyze. So look on the web
> for colour theory and how to select colours so the human eye doesn't feel overwhelmed."*

The first half of that is a genre diagnosis and it is correct. Every palette here had been
borrowing from code editors — dark ground, saturated accents, syntax-colour habits — and an
editor is a tool for scanning short lines of structured symbols. A scientist reading a two-page
answer needs the opposite: a room that holds still. The second half turned out to be the more
useful instruction, because the published guidance disagrees with what this repo had been
measuring.

### WCAG 2 is the wrong instrument for a dark theme

APCA's own documentation is blunt: WCAG 2 *"far overstates contrast for dark colors to the point
that 4.5:1 can be functionally unreadable"*, and *"cannot be used for guidance designing dark
mode"* — because reading performance differs between polarities at the same ratio, and one fixed
formula cannot model both.

Measuring the shipped palette in APCA found what the eye had already reported:

| | APCA | WCAG 2 |
|---|---|---|
| body text on background | Lc 82 | 12.8:1 |
| **accent on surface** | **Lc 47** | **6.1:1** |

APCA's levels are Lc 90 preferred for columns of body text, **Lc 75 the minimum**, Lc 60 the
minimum for content text that is not body text. The accent was at **Lc 47 — under the floor for
incidental text** — while WCAG called it a healthy 6.1:1 and every test passed. And this app
draws filenames and column names in the accent, so in the transcript that colour is not a button
label glanced at; it is text a researcher reads by the paragraph. Under-contrasted *and*
over-saturated at once: chroma 0.162 against surfaces at 0.025.

That is the whole of *"overwhelmed"*, in two numbers.

### What the sources actually say

- **No pure black, and no maximum contrast.** Pure black grounds cause halation — light text
  blooming into the ground — worst for the roughly half of readers with astigmatism, and reading
  speed drops measurably against a tuned dark grey. `#121212` is the commonly cited floor.
- **Desaturate on dark.** Saturated colours produce optical vibration against dark grounds. This
  is exactly the pink in the screenshot.
- **Off-white, not white, for long reading on light.**
- **OKLCH for the arithmetic.** Perceptually uniform, so an even lightness ladder *looks* even and
  equal chroma across a set means equal visual weight. This is why the old signals could all pass
  their tests and still fight each other: nothing had ever asked them to weigh the same.

### Both palettes are now solved, not chosen

Every value comes from a target rather than a judgement:

- **Body text at Lc 90** in both halves — the level named as preferred for columns of body text.
  Deliberately *not* higher: the very first cut ran at 15.3:1, near the maximum available, and
  maximum contrast is what makes text bloom.
- **Three ink roles at Lc 90 / 76 / 66** (dark) and 90 / 78 / 70 (light). The previous pair
  collapsed — `text_muted` and `text_faint` came out five hex digits apart and were the same
  colour to a reader.
- **Signals at equal OKLCH chroma** — 0.09 on dark, 0.11 on light — against print originals of
  0.15–0.24. Equal chroma is what stops any one status shouting.
- **Surfaces on four even OKLCH lightness steps** at chroma 0.018 (dark) and 0.006 (light); the
  same tint reads far stronger against paper than against near-black.
- **One CIP hue for the room and the accent.** Both are 2607 C purple's 308°, and what separates
  a panel from a link is *chroma alone* — 0.018 against 0.09. The four signals are 369 C, 137 C,
  1795 C and Process Cyan, each within 1° of the printed colour.

Two rules are now pinned. `apca_agrees_with_its_own_published_reference_values` checks the
constants against the published extremes, because every threshold is meaningless if that drifts.
`text_meant_for_reading_clears_the_apca_body_text_floor` holds body text at Lc 85 across all eight
palettes and the accent at Lc 75 for the Papa Nativa pair — verified by putting the old accent
back and watching it fail with *"accent is Lc 47 on surface"*.

The accent floor is asserted for **two** themes, not eight, and that is recorded rather than
quietly scoped: **Mini-Me Dark measures Lc 48 and Slate Lc 51**. Same defect, older palettes, and
a bigger change than this one.

### One disagreement, resolved by taking the stricter side

`text_faint` on the light half is the single value APCA did not settle alone. At the Lc it wanted,
it measured 4.4 against the orange-tinted row — under the WCAG floor this repo enforces. It ships
darker than APCA asks so that both scales pass. Where two measures disagree, the palette clears
the higher bar; neither gets discarded because it was inconvenient.

*Thirty-sixth: a test suite encodes which questions you thought to ask. Eight palettes passed
every assertion in this file while one of them was unreadable in the way a reader actually
noticed — because nothing had asked "is this comfortable", only "is this legible".*

## 184. Three pigments, three jobs (2026-08-13)

> *"When I hover on buttons my eye cannot distinguish the hovering and the background."*
>
> *"Maintain the pale orange for the conversations and the hovering must be the magenta potato.
> That theme must be named violet native potato, so we can create a third theme where we
> interchange the violet and magenta."*

### The hover was not faint, it was absent

`picker_row` hovered to `elevated`. `menu_card` — the card those rows are drawn inside — **is**
`elevated`. Same for the rail. So in those places the hover fill was not a small change, it was
**the same colour**, in every theme since this file was written, and no amount of squinting was
going to find it.

Where the two did differ it borrowed the elevation ladder, and an elevation ladder is built to be
subtle. The light potato's steps are **0.012 apart in OKLCH lightness**, about a third of what an
eye can find on a large flat area. Two different jobs had been sharing one set of colours: *this
panel is above that one* and *the pointer is here* need different amounts, and only one of them
should be quiet.

### The researcher's fix is better than the one being written

Mid-change, the instruction above arrived, and it resolved an objection already half-written into
the code. A hover tinted with the accent was rejected because **orange already means chosen** —
eight rows paint `accent_soft` when selected — so a hover wearing orange would claim a row was
picked when the pointer was only passing over it. The neutral lightness step being written
instead was the second-best answer to that problem.

Giving hover **its own pigment** dissolves it. Three CIP colours, three jobs:

| | |
|---|---|
| 2607 C violet | what you can act on |
| Process Magenta `#E6007E` | where the pointer is |
| 1505 C orange, 12% | the row you chose |

It is also more visible than the neutral step, because hue and lightness both move.

And it makes the palette's name mean something, which is why the pair was renamed and a second
pair added on the researcher's own construction — *"a third theme where we interchange the violet
and magenta"*. **Violet Native Potato** and **Magenta Native Potato**, each light and dark, named
by their accent, identical in every other value. The magenta accent is solved to the same Lc 76
on `surface` as the violet it replaces, so it is exactly as readable and no more.

### A rename is a silent theme change

`apply_theme` resolves a name and falls back to the default when it finds none — right for a
palette somebody deleted, exactly wrong for one that was renamed. Left alone, a researcher
reading on *Papa Nativa Light* would have opened the app next morning in the dark default, with
nothing said and nothing they did to cause it. `canonical_name` maps the retired names forward on
read, so the picker's tick, the dropdown's label and the palette actually painted cannot
disagree, and the new name is written back on the next save.

### Where the promise stops, and why it is written down

A hover fill is a **surface** for the length of the hover, and the AA sweep never saw it — so the
first version of the check found `text_faint` at 4.33 on the light potato's own hover. Fixed by
darkening that ink and the fill together, and both new palettes now clear 4.5 on every hover fill
as well as every surface.

Then the derived fallback was measured across the six older palettes, and there is **no fraction
that works**:

| | smallest lift that is visible | largest lift keeping inks at AA |
|---|---|---|
| Bench | 0.055 | 0.010 |
| Bench Night | 0.040 | 0.025 |
| Mini-Me Dark | 0.050 | 0.040 |
| Slate | 0.050 | 0.025 |
| Paper | 0.055 | 0.115 |
| High Contrast | 0.070 | 0.190 |

Four of them carry inks so close to the 4.5 floor that **every step large enough to see drops one
under AA**. So those six keep `elevated` and a weak hover rather than gaining an unreadable one,
`hover_over` promises nothing it cannot keep, and the test asserts only over the palettes that
name a fill — with a count, so a filter that quietly matched nothing could not make it vacuous.
Verified by pointing a potato hover back at `elevated` and watching it fail with *"changes it by
1.000"*.

- ⬜ **Four older palettes cannot show a hover** — Bench, Bench Night, Mini-Me Dark and Slate.
  Fixing them means retuning their inks for headroom, which is their own job and a bigger one.

*Thirty-seventh: two jobs sharing one colour is not a saving, it is a coincidence waiting to be
noticed. Elevation and hover were the same value here for as long as the file has existed, and
the day one theme made the ladder subtle, the other job simply stopped happening.*

## 185. Silence was carrying two meanings (2026-08-13)

The sources panel had a rule, and it was a good one: **only say something when something is
wrong.** A line under every reference confirming it checked out is fourteen lines of reassurance
nobody reads, and it buries the two that matter.

The rule answers *is this broken*. The trouble is what silence then meant. Three different facts
rendered identically:

- this came out of a search, and nothing in the link was composed;
- the model wrote a DOI down and Crossref agrees it names this paper;
- **the model wrote this from memory and nothing has confirmed it.**

The third is not an error. Barrera et al. (2016) came back real, relevant, and from a journal
Semantic Scholar indexes poorly — which describes a great deal of CIP's own literature. It is a
citation a **subject-matter expert has to settle**, and a researcher could not tell which ones
those were, because they looked exactly like the verified ones.

That is the thing org policy asks for, in its own words: *validate AI-generated content with
subject matter experts*, and *disclose when generative AI has been used in your work*. Neither is
possible if the app will not say which references it stands behind.

### Origin is a different question from Verdict

`references::origin(verdict, matched_in_registry)` answers **where did this come from**, and it
needs both arguments because neither settles it alone — a citation whose own DOI named the wrong
paper is registry-backed once Crossref finds the right one and unconfirmed when it does not, and
the `Verdict` is `Mismatch` in both cases.

Four answers, named for **what was done** rather than for what is true, which is the rule the
rest of that module already follows. `Unconfirmed` does not mean invented; most of these are real
papers, and saying otherwise with the app's authority is the §(references) mistake — *"does not
appear to describe a real paper"* about a correctly cited monograph.

`Pending` is the fourth and it is load-bearing. Reporting a reference as unchecked while its
lookup is in flight is precisely the bug that told a correctly cited Magurran 1988 it matched
nothing, mid-request. But a check that **failed** is not pending — nothing is coming back, and a
row stuck on *"checking…"* reads as verified to anybody who looks away and returns.

### Three places, one function

The header counts (`SOURCES · 14 · 3 UNVERIFIED`), the row says which, and the exported `.bib`
carries `annote = {unverified: this reference came from the model, not from a search}`. All three
read `source_origins()`, because three re-derivations of "unverified" from two maps is three
chances to drift apart, and a header that disagreed with its own rows would be worse than either.

The export matters most. The panel can be re-read; a `.bib` in somebody's Zotero is on its own,
and it is the copy that ends up in a manuscript.

### What this also closed

Every arm of the row's `match` answers *is this broken*, so falling through to `_ => None` meant
"nothing wrong" — and `(NoIdentifier, None)`, `(Unregistered, None)` and a source with no verdict
at all once resolution had stopped all landed there. Silence by omission rather than by decision.
Asking a second question with an answer in every case is what closes those, rather than adding
three more arms and waiting for the fourth.

*Thirty-eighth: when a display speaks only on failure, absence of a message is doing real work —
and it will quietly acquire every meaning nobody assigned it.*

## 186. A turn billed to a provider nobody chose (2026-08-13)

> *"This is weird, I set OpenRouter and I have credits."*

A turn failed with **"An internal error occurred"** and a pointer to the sidecar log. The log had
the real answer, and it was not the one the message implied:

```
openai.RateLimitError: 429 — 'You have no credits remaining. Add credits to continue
using the API at https://platform.openai.com/settings/organization/billing/'
```

**OpenAI.** The researcher had selected OpenRouter and had credits there. The request went
somewhere they had not chosen, and the first news of it was somebody else's billing page.

### How a request reaches the wrong provider

OpenRouter is not a pill of its own — it is reached through `custom` plus a base URL, which is
the documented design and is fine. What is not fine is the path when something is missing:

1. `model_choice` reads the key from the keychain under `llm:<provider>`. Keys are filed **per
   provider**, so one pasted while another pill was selected belongs to that one.
2. With no key, `run_request_body` omits the whole `__llm_keys` block — **and `base_url` lives
   inside it.** So the request carries neither a credential nor an endpoint.
3. The backend builds a bare OpenAI client, which falls back to whatever `OPENAI_API_KEY` the
   distro holds, and posts to `api.openai.com`.

Every step is locally reasonable. Together they turn a missing key into *a turn against a
different company's account*, several minutes later, reported as an internal error.

### It was already known, and deliberately not acted on

`problems()` has computed exactly this since §20 — *"No API key stored for X."* — and `main`
says what it did with it, in its own comment: **"Warned, not fatal: the app still opens, which is
where the user fixes it."**

That reasoning holds for *opening the app*, and does not survive being extended to running a
turn. There is no warning to read in a failure with no message. `misdirects_a_turn` now refuses
the turn and opens Settings › Model on the sentence that explains it.

**Only the two failures that are silent.** A wrong model id is not blocked: it fails loudly, at
the provider that was chosen, in a sentence naming the model — and refusing it would stop
somebody trying a model released this week, which §58 already decided against.

### Choosing a provider now takes a decision

> *"Also a modal that confirms the user when he sets the providers, and when he wants to change
> the providers there must appear a modal that the user must confirm."*

Which provider is selected decides **which account is billed**, and the only thing that said so
was which pill was lit — one click, no confirmation, no statement of consequence. The pill now
stages the change and a modal states the three facts a person needs, read from the keychain and
the settings rather than from what the panel happens to show:

- whether a key exists **for the provider being moved to**, said plainly *because keys are filed
  per provider* — which is precisely how this one went missing;
- that a custom endpoint needs its base URL, and what OpenRouter's is;
- which model id it is about to be set to, since changing the provider changes that too.

It mounts **above** the Settings pane and is dismissed **before** it, neither of which is
cosmetic: the pill that raises it lives inside that pane, so a confirmation drawn underneath
would be invisible, and an Escape that closed the pane first would leave the confirmation
orphaned over the workbench.

*Thirty-ninth: "warned, not fatal" is a judgement about one moment, and it does not travel. The
warning was written for a launch that could still be fixed, and then covered a turn where there
was nothing to read it in.*

## 187. The specialist was on a different account (2026-08-13)

> *"My hypothesis is that the subagents are not getting the default model at the top."*

Right, and the mechanism is worse than a subagent ignoring the default: **it had been given an
explicit one, from a different provider, with nothing on screen to say so.**

The settings pane showed the coordinator on **Custom (OpenAI-compatible)** with
`openai/gpt-5.4` — an OpenRouter id, correctly set, on an account with credits. Below it,
`academic_researcher` read **`gpt-4.1`**. The failing turn was a literature search, which
delegates to exactly that specialist. §186 had just fixed the coordinator's path and the turn
failed anyway, which is what made the hypothesis worth taking seriously.

### One list, five providers, and a slash

`specialist_list` offers every model from all five providers, flat. With the coordinator on
`custom`, that list contains:

| row | provider | account billed |
|---|---|---|
| `gpt-4.1` | `openai` | the OpenAI account |
| `openai/gpt-4o-mini` | `custom` | the OpenRouter account |

**A slash.** That was the entire visible difference between "runs where the rest of your work
runs" and "runs on a different company's bill".

The row *could* have said which provider it belonged to — the code computes it — but only
rendered it when the key was **missing**, on this reasoning, quoted from the line itself:

> *"Named only when it would be a second provider to key, since that is the thing a researcher
> has to act on before the choice can work."*

That is a defensible sentence about **whether a choice will function**, and this researcher had
an OpenAI key, so it functioned perfectly and billed the wrong account. The same shape as §186,
one screen down: the app said what was *broken* and never what it would *cost*.

### Every row that leaves the coordinator now says so

`specialist_note` annotates any model from a provider other than the one running the
conversation — keyed or not — and the two messages are deliberately different, because they ask
for different things:

- **`OpenAI — no key stored`** — fix this before it works.
- **`OpenAI — billed separately`** — know this before it costs.

`None` only for the coordinator's own provider, where the models are billed exactly where every
other turn is and a note on every row would be noise.

- ⬜ **Nothing stops a specialist being pointed at an unkeyed provider.** §186 refuses a *turn*
  whose coordinator has no key; an override to a provider with none still saves, and fails inside
  the subagent minutes later. The gate reads the coordinator's settings alone. Recorded because
  the fix is the same shape and the failure is slower and harder to read.

*Fortieth: "is this configured correctly" and "what will this spend" are different questions, and
a UI that only answers the first will let somebody spend confidently.*

## 188. Ask the provider what it has (2026-08-13)

> *"We need to have current available models updated from providers api… For the case of
> openrouter which have more models including opensource models like kimi or deepseek we should
> have a longer list."*
>
> *"The error happened when I selected gpt-4.1 even when from openrouter I should be able to use
> it. So I'm thinking that you are using the url from openai and not the one from openrouter."*

The second observation is the sharper one, and §187's account was true but incomplete. `gpt-4.1`
came from the **openai** provider's curated list, so the app sent `openai::gpt-4.1` and OpenAI's
URL — correct behaviour for that row, and not at all what the researcher meant. What they meant
was *gpt-4.1 through OpenRouter*, which is a real model with a real id: **`openai/gpt-4.1`**. It
was not offered, because the `custom` provider's curated list holds four entries and that is not
one of them.

So the fix is not a label. It is that this repo should stop guessing.

### The list comes from the provider now

`catalogue.rs` asks each provider's `/models` endpoint, caches the answer beside `settings.toml`,
and refreshes when it is a day old. §58 already said why a hand-written list could not hold —
*"a provider ships a model the day after a release"* — and that argument is merely uncomfortable
for Anthropic's four and fatal for a gateway carrying several hundred, including the open-weight
models a research centre has good reason to prefer.

Three decisions worth stating:

- **The provider replaces the curated list; it never merges with it.** A union would keep retired
  ids in the picker forever, and the provider is the authority on this.
- **Fetched when the Model pane opens, not on a timer.** A background poll spends a researcher's
  key on a request they cannot see, and the only moment the answer matters is when somebody is
  reading the list.
- **Silent on failure.** Offline, rate-limited, or a gateway that does not serve `/models` all
  mean *keep the list you have*. This is a nicety on top of something that already works.

What leaves the machine is one `GET` asking "what models do you have", carrying the key that
provider already receives on every turn. OpenRouter's needs no key at all, which is what lets its
catalogue arrive before anything is configured.

### Two things that fell out of it

**The rows were unreadable.** `picker_row` put the label and its note in one row with
`justify_between`, and since the label is the one that ellipsises, `gpt-4.1 · OpenAI — billed
separately` rendered as **`gpt-4.`** — *"I cannot read the complete model name."* They stack now,
which the gateway ids need anyway: `meta-llama/llama-3.3-70b-instruct` is not a thing to fit
beside anything.

**A filter stopped being optional.** Four names in a scroll box is a list; four hundred is a
haystack. The model picker gets the same fuzzy field the theme picker has, so `kimi` finds
`moonshotai/kimi-k2` — and says *"No model matches that"* rather than showing an empty box, since
a filter matching nothing and a provider returning nothing look identical otherwise.

*Forty-first: a curated list is a claim about someone else's product, and it is wrong from the
day it is written. The only question is whether anybody notices before the person who needed the
model that was missing.*

## 189. Vivid, and the valley in the middle (2026-08-13)

> *"The magenta in Magenta Native Potato Light is too dark. I think the colour is #ed028c… I want
> vivid colours not opaque ones."* Then, clarifying: *"The vivid colours I want them for the boxes
> not for the text."*

That clarification is the whole design. A colour used **as text** and a colour used **as a fill**
are held to opposite constraints, and the first version of this measured the wrong one.

`#ed028c` measures OKLCH L 0.62, C 0.253 — and against the light palette:

| | |
|---|---|
| as text, on `surface` | WCAG **3.44**, APCA **Lc 61** |
| as a fill, under this app's dark ink | WCAG **3.06** |
| as a fill, under white | WCAG **4.21** |

**Every one of those is under the floor.** That is not a limitation of the palette; it is a
property of that lightness. `#ed028c` sits in the valley where a colour is too dark to carry dark
text and too light to carry white — the one place nothing can be read.

### What ships

One step darker, at the researcher's own hue and chroma, so an ink can sit on it: **`#d23482`**
(white at 4.61) and **`#a63deb`** (white at 4.68). Chroma **0.20 and 0.25**, against the 0.05 the
tints had — four to five times more colour, which is what *vivid* was asking for.

`ink_on` picks the ink by measuring the fill rather than assuming white. Measured per fill, not
stored per theme, because the answer differs across the four surfaces a row can sit on and an
imported palette has no such field. A fill the page's own ink already reads on **keeps it** —
flipping to white on a pale tint would be a change nobody asked for and a worse-looking one.

### The label had to stop naming its own colour

`ui::Label::colour` writes the colour onto the element, and an element's own style beats a
parent's refinement — the same rule that stops `text_color` reaching an SVG child (§157). So a
label that names its colour can never change with the row it sits in, and a hover that flips its
ink was impossible while every row did exactly that.

`Label::inherit()` paints no colour at all, and the row states both: the resting colour on
itself, the hover colour in the refinement. That is the only arrangement in which the two can
disagree, which is what a state *is*.

- ✅ **Scrolling a list inside the settings modal no longer scrolls the modal too** — reported
  fixed by the researcher on 2026-08-13.
- ✅ **The sources panel lists four and opens the rest** (§194). **Awaiting eyes.**

*Forty-second: ask what a colour is *for* before measuring it. Text on a fill and a fill under
text are different questions with different floors, and I answered the wrong one first.*

## 190. Three constraints, and only two of them fit (2026-08-13)

> *"To the vivid boxes put an alpha around 0.7."*

Measured, that asks for three things at once against a near-white page: **saturated**, **70% over
the surface**, and **an ink that can be read on the result**. Any two hold. All three do not.

At 0.7 the §189 fills composite to white-at-3.03 and dark-ink-at-4.25 — back in the same valley
`#ed028c` was in, one step along. Solving for a base whose composite carries the *page's* ink
collapses straight back to the pale tints the request was trying to leave. So the base moves down
instead, and the composite carries **white**: `#bc4b8d` and `#a64bd2`, chroma 0.19 and 0.24 on the
base. Softer than §189, still four times a tint.

### And a regression, shipped by me

§188 stacked `picker_row` into a column so a note could not eat the model name. It also set
`items_start`, which sets the cross axis to **content width** — and `Label::ellipsis` works by
growing to fill a width and then truncating to it. With nothing to fill, every row truncated to a
bare `…`, and the specialist picker became a list of ellipses: *"I can't select models for the
subagents."*

That is §59 exactly, one axis over, and §184 hit the same rule from the other side (*"`items_start`
leaves children at content height, so a `flex_grow` child has nothing to grow into"*). Three times
now. The rule, stated so it can be looked up: **`items_start` and `flex_grow` are opposites, on
whichever axis they meet.** One says *be your content*, the other says *be your container*.

*Forty-third: a fix that stacks a row changes which axis every child was relying on, and the child
relying on it was two files away.*

## 191. A key belongs to a company, not to a conversation (2026-08-13)

> *"Check how Mini-Me's frontend structures the settings for the providers… Zed has a modal to
> manage AI providers. Analyse and select the best, or join ideas."*

Both references were read, and they agree — which made the decision easy rather than a matter of
taste.

**Mini-Me's own web panel** (`ModelConfigPanel.tsx`) lists every provider at once with a
*Connected* badge, an Add-key / Disconnect control per row, and a `N connected` summary. Its
subagent picker uses `<optgroup label={provider.name}>` with `{m.label} · {m.ctx}`.

**Zed's LLM Providers page** is the same shape: one page, every provider, each with its own API
key field and its own status.

Neither makes you *switch to* a provider in order to configure it. This app did, and that was the
whole defect: `Settings::key_name()` is `llm:<selected provider>`, so filing an Anthropic key
meant selecting Anthropic, pasting, saving and switching back — with §186's confirmation
interrupting each hop. Meanwhile the request path had supported a key per provider since §20:
`extra_keys` gathers one for every provider a specialist names, and the backend derives the same
set from the specs. **The plumbing was there and the pane was the bottleneck.**

### What was taken from each

- **From both: every provider visible at once.** A row of chips above the key field, each ticked
  where a key is filed, so *which of these am I missing* is one glance rather than five clicks.
  Choosing one retargets the field and **nothing else** — the coordinator does not move, so no
  confirmation is owed and none is shown.
- **From the web panel: the company's name once, over its models.** The specialist picker now
  groups under provider headings. That is not only tidiness: it returns the width that
  `OpenAI — billed separately`, repeated on every row, was taking from the model id — which is
  the same width `meta-llama/llama-3.3-70b-instruct` needs.

`key_target` is separate state from `draft.provider` for exactly the reason the references imply:
a key is a fact about an account, and which conversation is running is a fact about right now.

*Forty-fourth: when two independent designs converge, the argument is over. The interesting
question is what they both refused to do — and neither of them makes you switch context to
configure something.*

## 192. The same flex rule, from the third side (2026-08-13)

> *"I cannot see the model so I cannot select! Each subagent selection must have a filtering as
> the main model."*

§190 diagnosed the bare `…` rows as `items_start` giving children content width, removed it, and
was **wrong** — or rather, right about a real problem and not about this one. The rows were still
ellipses afterwards.

`Label::ellipsis` is `flex_grow().truncate()`, and the comment on it says exactly why:
*"`flex_grow` is what gives it a width to truncate to."* True **in a row**. §188 turned
`picker_row` into a **column**, and in a column `flex_grow` grows the *height* — the width stays
at content, and content width for a truncating label is nothing.

So the same rule has now produced three different bugs from three different directions:

| | |
|---|---|
| §59 | no `flex_grow` in a row → bare `…` |
| §184 | `items_start` in a column → a `flex_grow` child with no height |
| §192 | `flex_grow` in a **column** → a label with no width |

The fix is `w_full()` *and* `flex_grow()`: the first supplies width in either direction, the
second still does §59's job where the parent is a row. One label that works in both, rather than
a rule about which container it is allowed to sit in.

### And the filter the grouping made necessary

Grouping under provider headings (§191) made the list *longer*, not shorter — four catalogues
stacked instead of one. The specialist picker gets the same fuzzy field the coordinator's has, and
shares the same entity, since only one picker is open at a time and both ask the same question of
the same catalogue. A provider whose whole catalogue filters out drops its heading too, or the
result is a column of company names with nothing underneath them.

*Forty-fifth: a comment that says why a line is there is only true of the container it was written
in. "`flex_grow` gives it a width" was correct, load-bearing, and became false the moment
something above it changed direction.*

## 193. Settled by comparison, not by reasoning (2026-08-13)

> *"The ellipsis is not fixed."* — third report, after §190 and §192 each fixed a real thing that
> was not this.

Three attempts, three plausible mechanisms, three wrong answers. What settled it was not a fourth
theory but a **comparison already on screen**: the provider headings added in §191 sit two lines
away in the same list, are the same `ui::Label`, and render their text correctly. The only
difference between them and the rows is `.ellipsis()` — `overflow_hidden` + `whitespace_nowrap`
+ `text_ellipsis`.

So the truncate path is removed from `picker_row`. A row that is already a column has somewhere
to put a long id: it wraps. Wrapping is worse than truncating and **enormously** better than
showing nothing, which is what three rounds of clever fixes had shipped.

The rule that failed here is not a flex rule. It is that I kept reasoning about layout I could not
see, in a window I cannot open, while a working counter-example sat in the same function.

### Softness, matched to the orange rather than chosen

> *"If you see the softness of the orange, we should simulate that softness for the potato light
> themes."*

The orange is CIP 1505 C at **12%** over the surface, and that number was already in the file. The
potato hovers now use the same 12% of their own pigment — `#f9dbef` and `#ecdbf6` — which is the
literal answer to *simulate that softness*, and it means the fills stop needing an ink flip: dark
text reads on them at 5.01 and 4.89.

§189's vivid fills and §190's 70% composites were both answers to *"vivid"* that arrived before
the reference point did. The orange was the reference all along.

*Forty-sixth: when three fixes fail, stop generating a fourth explanation and go looking for the
thing that already works. It is usually adjacent.*

## 194. The reference list is a list, not a slideshow (2026-08-13)

> *"Let's change the UI of sources like we did for the images. It's more ordered rather than
> showing a long list of papers. And when the user clicks, instead of a sliding visualiser he can
> see a nice list that can scroll in y direction. Like OS systems do in file explorers."*

Twenty-six references rendered in full is a wall the researcher scrolls past to reach the files
underneath — the same problem the images had before §152 put them behind one tile. The panel now
shows **four** and offers `+22 more · open all`.

**And deliberately not the slider the images got.** That distinction is the request's, and it is
right: a figure is one thing you look at and the next is a different thing, so paging suits it. A
reference list is one object you read *down*. Paging through citations one at a time would be the
wrong gesture for the same reason paging through a folder would be.

So the modal is a scroll region, 720px wide, holding every reference in the same rows the panel
draws — **the same function**, called with `None` instead of `Some(4)`. Two renderers would be two
places for the unverified mark or the link to go missing from one and not the other, and §185 put
that mark there precisely so a researcher could trust its absence.

The footer carries §185's count in words rather than as a number: *"3 of these came from the model
rather than from a search — confirm them before citing."* Same function as the header, so the two
cannot disagree.

*Forty-seventh: two collections that look alike on screen can still want opposite gestures. What
decides is whether the items are alternatives to each other or parts of one thing.*

## 195. The citation is the target (2026-08-13)

> *"I would like to have a hover colouring when I'm hovering a paper, so when I click it I'll be
> redirected to the web page of the paper."*

A twelve-pixel word reading `link` at the end of a four-line citation is a target you *aim* at.
The citation is the thing being pointed at, so it should be the thing you press — and §194 made
that plain by turning the list into something you read down, where the rows are the objects and
the word at the end of each is furniture.

The whole row now opens the paper and lights up to say so, using the same `hover_over` fill every
other list uses, which since §193 is the orange's own 12% softness.

**Only where there is somewhere to go.** A reference nothing could resolve gets no hover and no
pointer: a row that lights up and then does nothing is worse than one that never offered, and
§185 already marks those as unverified in words. The `link` word stays, because it says *before*
you hover which rows have a destination — the hover confirms it, it does not announce it.

*Forty-eighth: an affordance placed beside the content is a second thing to find. Making the
content itself the control removes the search.*

## 196. Restored the buckets and forgot the sources (2026-08-13)

> *"When I reload the conversation, I cannot see the interaction of sources."*

Two lists render the same references, and only one of them survived a reload.

`open_conversation` restores `buckets`, `project`, `jobs` and `tasks` from the reopened
snapshot — and not `sources`. So a reopened conversation showed the **generic bucket
rendering**: a name, a count, and `+13 more`. Everything §185 through §195 built lives on
`self.sources` — the unverified count, the provenance note, the link, the row you can press — and
all of it was simply absent.

What made this hard to see is that the two look **similar enough to be mistaken for each other**.
Both say "sources" and a number. The bucket version is not obviously a fallback; it is just a
quieter list. So the symptom read as *the feature is broken* rather than *a different renderer is
showing*.

`reports` was missing from the same block for the same reason, and is restored with it.

- ⬜ **Two renderers for one collection.** The research panel draws `sources` from `buckets` when
  the structured list is empty, and from `self.sources` when it is not. That is now correct in
  both states and still two code paths for one thing — which is how this happened. The bucket
  fallback should defer to the structured list rather than sit beside it.

*Forty-ninth: a fallback that looks like the real thing is worse than one that looks broken. Six
sections of work were invisible and the screen looked fine.*

## 197. A filter for the reference list, and the flake I kept explaining away (2026-08-13)

> *"Add a search bar in the modal panel when I open the sources so the user can filter."*

The open list gets the same fuzzy field every other list here has, scored against **the citation
as written** — which is what a researcher remembers: an author, a year, a word from the title.

Two details that are not obvious:

- **The field sits outside the scroll region.** `Modal::body` is itself a scroller, so a filter
  placed inside it scrolls away from the list it filters. The inner `max_h` means the body's own
  content fits and only the list moves.
- **References are numbered before filtering.** `[3]` has to keep meaning the third reference of
  the answer, because the prose points at it. A filtered list that renumbers is a list that
  disagrees with the text above it.

The panel's four are deliberately not filtered: a filter over a preview showing four of seventeen
is a filter whose result you cannot see.

### And the flake, which was mine

A test had been failing about one run in four for the last several sections, and I called it *"a
stale build"* three times without proving it. It is not. **`apply` writes the live theme to global
atomics** — the whole design of this file, since free rendering helpers have no `Context` to reach
a GPUI global through — and `cargo test` runs tests in parallel. Three tests call `apply` and then
read `text()` or `ink_on()`. Whichever lost the race failed.

A `THEME_LOCK` now serialises them, taken as the first statement of each, and recovering from
poisoning so one panicking test does not fail the other two for a different reason. Five
consecutive clean runs.

Worth saying plainly: I attributed a real race to a build artefact three times because the rerun
was always green. **A test that passes on retry is evidence of a race, not evidence of nothing.**

*Fiftieth: "it passed the second time" is the beginning of an investigation, not the end of one.*

## 198. The result is already on disk (2026-08-13)

> *"When a background task has a success, we should see a modal button that the user can press
> and that serves as a check status, so the user doesn't type it every time in the chatbox."*

Asked which of two things the button should do — open what the worker produced, or send the
question on the researcher's behalf — the answer was **open what it produced**. That is the
cheaper and the truer one: a finished worker's output is *already on disk*, and asking a model to
describe files it wrote ten minutes ago costs a turn to get a worse version of what is sitting in
a folder.

`worker_dir` finds it. A worker runs on its **own** LangGraph thread but writes **inside its
parent's folder** — the overlay composes `[conversation_thread, worker_thread]` when the two
differ, which is what §151 verified live, with plots at `<task>/guinea_pig_eda_output/plots/…`
rather than in a sibling directory nobody would think to open.

Three things it does not do, each on purpose:

- **No turn, no model call, nothing billed.** The button is instant because the answer already
  exists.
- **Falls back to the conversation's folder** when the worker wrote nothing of its own. A button
  that opens a directory which does not exist is worse than one that opens the folder above it
  and lets somebody look. A *file* of that name is not a folder either, and is not offered as one.
- **Names the specialist** — *"Show what exploratory data analysis produced"*. Several workers run
  at once (§43), and a column of identical buttons is one you have to count rows to use.

*Fifty-first: before building a way to ask for something, check whether it is already sitting
somewhere. The fastest request is the one that turns out to be a lookup.*

## 199. Who made this, and what are we here for (2026-08-14)

> *"I don't know the names of the subagents that produced this output. The name of the subagent
> must appear in the provenance so I can know the path of work. This is related to: I cannot
> modify the project mission."*

Two complaints, one screenshot, and they really are related: both are the panel showing a fact and
withholding the thing behind it.

### The files did not say who wrote them

`FILES · 26`, then `15 images`, then `Background task files`. Three headings, none of which names a
producer. §152 had removed the 36-character UUID from the folder heading as app bookkeeping, which
it is — but that UUID *is* the attribution. The background worker runs on its own thread and writes
into a folder named after it, so the folder is the record of who produced the file, and stripping
it left the only exact provenance the client has on the cutting-room floor.

So the worker's name **takes the UUID's place** rather than disappearing with it. `background
worker / plots`, not `plots`; `5 images from background worker`, not fifteen images from nowhere.

And the image tray had to stop being one tray. §152 put every image in one grid because images are
what a person opens the panel to look at — right within one body of work, wrong across two. A
researcher reading *"15 images"* was looking at the conversation's plots and a worker's plots in
one strip with no boundary drawn. `by_producer` now splits before `split_images` does, so
"together" means together *with the rest of that job's output*. A background worker already has its
own job row, its own folder and its own button; its figures are a separate body of work by the same
argument.

**What is deliberately not claimed.** The specialists consulted inside the conversation —
`exploratory_data_analysis`, `academic_researcher` — share the conversation's thread and its one
directory, and nothing on the wire says which of them wrote a given file. Matching mtimes against
the road strip's arrival windows would produce an attribution for every file and would be a guess:
a specialist can hand a filename to the coordinator that writes it, and two can overlap. That is
`provenance.rs`'s own rule from §73 — *a provenance record that quietly guesses is worse than none,
because it will be believed* — so the conversation's own files stay unlabelled, which is what
"unlabelled" should mean. A thread with no matching task says `a background task`: the folder
proves a worker wrote it even when the task list did not survive the reload, and "we don't know
which" is not "nobody".

### The mission could not be changed

`PATCH /project` has existed the whole time. Its own docstring says what it is for — *"let the user
read and edit it by hand — rename the mission, add a backlog item"* — and this client had never
called it. The panel rendered `project.mission` as text with nothing to press, so a researcher
whose opening question was a warm-up was stuck with it.

Not stuck with a label, either. Two facts from the backend make this the difference between an
annotation and a steering wheel:

- `advance_project` seeds the mission from the first human message **only when it is empty**
  (`backend/project.py:373`), so a hand-set one survives every later turn rather than being
  overwritten by the next question.
- `ProjectSpineMiddleware.awrap_model_call` injects the mission **into the coordinator's system
  prompt** (`backend/middleware/project.py:136`), so editing it changes what the agent does — how
  it delegates, what it passes the planner — and not only what the panel displays.

Three decisions in the wiring:

- **The write is scoped exactly like the read.** One `project_url` for both, because the overlay
  wraps `get_project` and `patch_project` alike and a PATCH that spelled its scope differently
  would save into a namespace the panel never reads — a save that looks like it silently did
  nothing. There is a test for that and nothing else about the request.
- **Not optimistic.** Renaming a conversation shows the typed name immediately because the name is
  ours. A mission is the backend's: capped at 500 characters, runs of whitespace collapsed. The
  panel renders what the store returned, or says why it could not — a silent failure here leaves a
  researcher believing they redirected the agent when they did not.
- **The `Edit` control lives in the heading, not on hover.** The mission text was already the
  button; the affordance simply did not exist until the pointer was on it. Our researchers are not
  developers, and an invisible affordance is not an answer to someone who has already concluded
  there isn't one.

*Fifty-second: when the client can't attribute something, say the smaller true thing. "A background
task" is worth more than a confident guess and costs nothing to be right about.*

## 200. The line nobody broke (2026-08-14)

> *"When I write a long text the box doesn't increase in height, which causes I cannot see what
> I'm typing."*

§55 made the composer multi-line by splitting on `\n` and shaping one line per segment. `shape_line`
lays out exactly one line and takes no wrap width, so a paragraph typed without pressing
shift-enter was **one row** — shaped at its natural width, painted from the left edge, and clipped
by §97's content mask at the right. The caret went out with the text. The field stayed one line
tall while it happened, because the height was `newlines + 1`.

Every part of that was working as written. The gap is that *nobody types their own line breaks in
a research question*, so the only case that mattered in daily use was the one case the feature had
never handled.

**Wrapping in the string domain, not the layout domain.** `shape_text` returns wrapped lines with
their own coordinate space; adopting it would have meant rewriting caret placement, per-row hit
testing, the selection quads and the IME rectangle in one change. Instead `LineWrapper` — the same
one gpui's own text element uses — is asked where each hard line has to break, and each resulting
row is shaped on its own. Downstream, `lines` is still `Vec<(byte offset, ShapedLine)>`; there are
simply more of them. Nothing else in the element changed.

**The height is measured against the previous frame's width.** This is the one thing here that
looks like a shortcut. Wrapping needs a width; the width is only known once layout has run. The
alternative is `request_measured_layout`, which means giving up the `width: 100%` + `flex_grow` +
`min_width: 120px` triple in `request_layout` — and this file has paid for that combination four
times (§72, §88, §97, §99), each time with a field collapsed to a sliver and a placeholder painting
out the side. The cost of using last frame's number is one stale frame while a window is being
dragged wider, and a drag is a stream of frames. That trade is written down at the call site.

**Past eight lines the box moves instead of growing.** The cap was already there; what was missing
is that a capped box has to *follow the caret*, or a researcher on line twenty is shown lines one
to eight. `first_row` is now remembered on the composer, because both conversions between screen
and offset — mouse hit-testing and the IME rectangle — have to subtract it. A scrolled field that
forgot how far it had scrolled would put the caret eight lines from where the click landed.

The wrapping itself is gpui's and needs a window to measure. What this module can get wrong on its
own is turning break points into ranges over the whole string, so `row_ranges` takes the breaks as
a closure and the tests supply a fake one. Two of the three cases they cover are the degenerate
ones: a break at the very end, and a break at zero, each of which would add an empty row that
draws as a blank line nobody typed and pushes every row below it down.

*Fifty-third: a feature that handles the general case and not the common one is a feature nobody
has. "Multi-line" meant the line breaks we could see, and the ones we couldn't were the only ones
being typed.*

## 201. Ask the process that did the writing (2026-08-14)

> *"And yes we need to record the write, so maybe we can do that change."*

§199 named the background worker and stopped, honestly, at the specialists. `exploratory_data_analysis`
and `academic_researcher` share the conversation's thread and its one directory, and nothing on the
wire says which of them wrote a given file. The client could have matched file timestamps against
the road strip's arrival windows and produced an attribution for every file; it would have been a
guess, and `provenance.rs` refuses guesses on the grounds that a provenance record that quietly
guesses is worse than none, because it will be believed.

The answer is not a better inference. It is asking the only party that was there. **The backend is
the writer.** It knows which delegation it is inside, because the `task` tool was handed the name;
and it knows which files a command produced, because it started the command and can look at the
directory afterwards.

`overlay/minime_local/authorship.py` writes one JSON line per file into `.authorship.jsonl` in the
conversation's workspace — dot-prefixed, so the client's output scan, which already skips hidden
names, never lists the record as one of the files it explains.

**Two write paths, and the second is the one that matters.**

- `write` / `upload_files` are deepagents' file tools; the path is the argument.
- `aexecute` is a shell command, usually a Python script drawing plots. The desktop app's own
  comment has said for months that *a file written by a script inside `execute` registers no
  artifact, and those are most of them*. So the workspace is walked after the command and anything
  newer than its start belongs to whoever issued it. This is not the timestamp inference rejected
  above: one process takes both readings, from the same clock the filesystem stamps with, around a
  command it started itself. The interval is proven, not reconstructed.

**Three things it declines to do.** It never descends into a nested thread folder — a background
worker writing while the coordinator runs a command is the one case where two authors are genuinely
active in one tree, and §199 already answers for those files. It caps one scan and *says so in the
log* when the cap bites, because "no line in the manifest" and "there were too many files to look
at" produce the same empty panel. And it swallows its own failures: losing a line of provenance is
a worse panel, while raising is a turn that died after writing a file successfully — §18's rule
about what an overlay may risk.

**A `ContextVar`, not a global.** LangGraph schedules concurrent tool calls as asyncio tasks and a
task copies the context at creation, so each delegation's name is visible only inside its own
subtree. A module global would have the second specialist overwrite the first and both sets of
files come out wearing one name. Same shape, same reason, as `spine.py`'s `_http_project` — and
the async wrapper is `async def` with the `await` inside the block, which is the mistake that file
paid for: a sync wrapper sets the variable, builds the coroutine, resets, and hands back something
unawaited, so the name is gone by the time the subagent runs and the patch looks installed while
doing nothing.

**On the client, the folder outranks the manifest.** Inside a worker's own run the manifest records
that worker's coordinator, so reading it first would rename `background worker` to `coordinator` —
true of the inner graph, useless to the person reading the panel. Two rules, each exact in its own
domain, in a fixed order. The manifest is re-read only when its size or mtime has moved: it is
append-only and grows by a line per file, and parsing it on every frame of a streaming answer is
the disk I/O `shape_of` is cached to avoid.

Everything without a record stays unlabelled, exactly as before. Conversations that ran before this
existed have no manifest, and a backend without the overlay armed never writes one; both keep §199's
behaviour rather than acquiring a wrong answer.

*Fifty-fourth: when the client cannot know something, check whether the server was standing right
there when it happened. Two rounds of this were spent making inferences on the wrong side of the
wire.*

## 202. The caret had nowhere to be, and the warning cried wolf (2026-08-17)

Two defects from one testing session, neither of them the feature under test.

### The caret on the border

> *"If you see the image, you'll see that the bar `|` is beyond the text."*

The screenshot shows §200 working: text that was one clipped line is now two rows and the box grew
to fit them. What it also shows is the caret sitting on the box's right border, a couple of pixels
outside the text — which reads exactly like the defect §200 was supposed to fix.

`LineWrapper` was asked to wrap at `bounds.size.width`, so a full row ends flush with the inside of
the border. The caret is painted **after** the last glyph. At the end of a full row it therefore has
nowhere to go. Four pixels of reserved width fixes it, shared by the height calculation and the
shaping so the two cannot disagree about where a row ends.

The second half is subtler and would have shown up as soon as the researcher clicked mid-text: at a
wrap, row N's end and row N+1's start are the **same offset**. The old rule returned the first row
whose end reached the target, so a caret at that offset drew at the far right of the row *above* the
character it precedes. A typed newline consumes a byte and the two offsets differ, which is what
tells the cases apart — and for a typed break the caret does belong on the upper row, where the key
was pressed. `caret_row` now reads its answer from the same rule, so the row the box scrolls to and
the row the caret is drawn on cannot disagree.

*What settled it:* not reasoning about the wrapper, which is where the previous two composer
diagnoses went wrong (§190, §192). The screenshot showed two rows where there had been one, so
wrapping was working; the only thing left in the picture was where the caret was drawn.

### The warning that cried wolf

The same log carried this three times, after the researcher had killed the old backend and watched
this app start a new one:

> *attached to a backend that was already running — it is on the code it started with, not what
> this app now ships.*

`ensure_running` is called **once per turn**, not once per launch. It warned whenever the health
check passed, with no regard for who had answered it — so every turn after the app spawned its own
sidecar reported it as a leftover. §130 wrote that warning because the failure it describes is
invisible and expensive; saying it when it is false spends the same credibility in the opposite
direction, and it told a researcher whose restart had worked that their restart had not. It also lit
the amber banner in the Setup pane for the rest of the session.

`self.child` already knew. `try_wait` rather than `is_some`, because a child that died leaves its
handle behind and whatever answered the health check after that is genuinely not ours — and an
error means we cannot tell, which is the same as not knowing.

*Fifty-fifth: a warning that fires when it shouldn't costs exactly what a missing one does. Both
teach the reader to disbelieve the log.*

## 203. Provably right on paper, wrong on the window (2026-08-17)

§202 fixed the caret two ways and shipped both to a real Windows machine for verification. Four of
five cases passed. The one that failed was the one the fix was *for*:

> *"Clicking immediately before `has` on the lower wrapped row drew the caret at the right edge of
> the upper row, after `row`."*

The rule was right. Its inputs were not verifiable.

**What the rule needs.** At a soft wrap, row N's end and row N+1's start are the *same* byte offset —
no character was consumed to make the break. A typed newline consumes one, so the two differ. That
single comparison is the whole distinction, and the two cases want opposite answers.

**What it was comparing.** `start + ShapedLine::len()` against the next row's start. `len()` is
exact on Windows — `direct_write.rs:666` sets it to `text.len()` — and traced on paper the redirect
fires. On the machine it did not. Reading gpui twice did not explain it and a third reading would
not have either: this is the same dead end as §190 and §192, and §193 already recorded what to do
instead of arguing with code you cannot watch run.

So the quantity was removed rather than explained. The **ranges are already known exactly** — they
come out of the wrapping, before any shaping happens — and they were being thrown away and then
reconstructed from a shaped line one layer down. Rows now carry `Range<usize>`, and `caret_row` is a
free function over ranges alone: no shaping, no window, and every case the machine exercised is a
unit test, including the one it failed on.

That also made two other conversions exact. `index_for_mouse_position` and `position_for_offset` both
did the same `start + len()` arithmetic, so both had the same latent dependency, and
`position_for_offset` had a second copy of the boundary rule that could drift from the first.

*What this cost:* one round trip to a machine with a screen, because the failing case cannot be
reproduced here. Worth naming as a standing constraint rather than an accident — this project's UI
defects keep being ones no test on this machine can see, and the cheapest response is to shrink what
depends on the parts that need eyes.

*Fifty-sixth: when a rule is right and the answer is wrong, stop re-deriving the rule and look at
what it is reading. A correct comparison over a value you cannot verify is not a correct program.*

## 204. Four reports, and one of them was my test being wrong (2026-08-17)

### `End` was never a row's key

> *"This doesn't work: type a line, shift-enter, then press End on the upper line. The caret must
> stay on the upper row."*

It didn't, and §203 is not why. `End` was `move_to(self.content.len())` — the end of the whole
text — and `Home` was `move_to(0)`. So pressing `End` anywhere put the caret on the last row, which
is exactly what was reported and has nothing to do with the wrap-boundary rule. **The test
instruction was wrong**, and it was written by someone who had read the file that morning.

Worth naming as a failure mode of its own: a verification step that asserts the wrong thing costs
the same round trip as a bug, and it spends the tester's trust rather than mine.

`Home` and `End` are now the row's, with `ctrl-home` / `ctrl-end` for the document — which is what
every other multi-row field reserves them for.

### Up and down were never bound at all

> *"I don't have a scroll bar in the chatbox so I cannot go to the beginning."*

The composer had `left`, `right`, `home`, `end` and no vertical motion. That was survivable while a
row meant a typed newline; §200 made a long prompt several rows and §202 capped the box at eight, so
a researcher eight rows in had no way back to row three. Not a missing scrollbar — a missing arrow
key. A text field's answer to "take me back up there" is `up`, and this one had none.

**Scoped to `Multiline`, not to `Composer`.** The command palette binds `up`/`down` to move its
highlighted command, and its query field *is* a `Composer` nested inside the palette — so a
`Composer`-scoped binding sits deeper in the dispatch tree and wins, quietly taking the palette's
arrow keys away. Only a field that can hold a paragraph adds the identifier: the chat composer and
the mission editor. Caught before shipping by looking for other `up` bindings, which is the check
§84 already paid for once.

### The app's own bookkeeping was in the FILES panel

> *"I think we shouldn't show the provenance json file."*

`provenance.json` is the road strip's record, written beside the conversation's files so it survives
a reload. The output scan already skips hidden names, `__pycache__` and `memories` for exactly this
reason — §173's argument was that a researcher reading a panel headed FILES will open, edit or delete
what they find there. It just never listed this one. Now it does.

### The status nobody was reading

> *"We have a big bug. If I don't ask about the status the app is not checking the success or
> failure. If I asked, the success appears even when the agent already finished his work."*

The watcher is alive — it polls every four seconds and the panel's `running · data cleaning` proves
it, because that activity string comes from the same request. What it cannot do is notice the end.

`thread_state` derives status from `next`: empty means done, non-empty means running. It is written
`state.get("next").and_then(as_array).is_some_and(is_empty)`, so a **missing** `next` reads as "not
empty" and resolves to `running` — for as long as the app is open. The coordinator's
`check_async_task` reads a different source and gets it right, which is precisely why asking works
and waiting does not.

Whether that is the mechanism cannot be settled from a machine with no backend on it, and §203 was
one round trip too many spent guessing. So the value the argument needs now goes **in the log**,
naming what the payload did carry, the moment `next` cannot be read. Fifth time in this project that
the missing evidence was a value the program already had (§99, §91, §110, §114, §116). The fix
follows the log line, not the other way round.

*Fifty-seventh: check your test instructions against the code as carefully as the code. A wrong
assertion and a wrong implementation are indistinguishable from the far end of the round trip.*

## 205. One offset, two right answers (2026-08-17)

> *"When I'm on the last line, pressing home and end works as expected. But when I'm at the
> beginning of the prompt, pressing end directs me to the beginning of the last line."*

Both halves of §204 were doing exactly what they were told, and together they were wrong.

`End` goes to `row_bounds().end` — row 0's end offset. §203's rule then draws that offset on the row
*below*, because at a soft wrap **row N's end and row N+1's start are the same byte offset**. So the
caret went where `End` sent it and appeared where the rule put it, one row further down than anyone
asked for.

This is the affinity problem, and it has no answer in the offset alone. A wrap boundary has two
correct screen positions and the right one depends on how the caret arrived:

- **Downstream** — the lower row's start. Where a click on the lower row's first character belongs,
  and where an inserted character lands, so it stays the default.
- **Upstream** — the upper row's end. What `End` means by "the end of *this* row", and where
  vertical motion belongs when it lands on a row end.

`Affinity` is now carried on the composer, reset inside `move_to` so it has to be *asked* for and no
later cursor move can inherit a stale one, and set immediately afterwards by the two operations that
mean a specific row. `caret_row` takes it and uses it for one thing only: breaking the tie. Every
non-boundary offset returns the same row under either value, which is what stops it becoming a second
rule that can disagree with the first — and there is a test that asserts exactly that.

A typed newline is not a tie at all. It consumes a byte, so the two offsets differ and there is
nothing to break; `End` before a shift-enter stays put under either affinity, also tested.

*What this cost:* nothing extra, because it arrived in the same round trip as the arrow keys it was
found by. Worth noting the shape though — §203 removed an ambiguity from the *inputs* and this one was
in the *question*. A shared boundary offset cannot be resolved by being more careful about ranges; it
needs a second piece of information, and there was nowhere for it to come from until an operation
existed that had an opinion.

*Fifty-eighth: when two correct rules combine into a wrong answer, the missing thing is usually
intent — not precision.*

## 206. The log that was never being kept (2026-08-17)

Twice in one session a diagnostic was added, the researcher was asked to grep for it, and the answer
was empty:

```
PS> Select-String -Path ...\mini-me-app.log -Pattern 'no usable'
PS>
```

The second time that emptiness was taken as evidence — *"`next` is present, so the missing-field
theory is wrong"* — and it sent the next diagnosis down a path chosen by nothing at all. It was
caught only by a follow-up check on the same file, which found no lines from that session either.
The file was from an earlier attempt with `Tee-Object`; the run being asked about had never written to
it.

**The app kept no log.** `backend.rs` has always written the sidecar's output to
`%TEMP%\mini-me-desktop-backend.log`, and the app's own `tracing_subscriber` wrote to stderr only —
which for a windowed program means a console the researcher closes. Every `warn!` this project has
added for a researcher to read has been going somewhere nobody keeps.

So the app writes its own, beside the backend's, and three details are the point:

- **Both destinations, not either.** A `Tee` writer: the console for whoever is watching live, the
  file for whoever reads it afterwards. The console's result is the one returned, so a locked or full
  file can never cost a line someone is watching arrive.
- **Truncated per launch.** A researcher told to read this file must be reading *this* run. An
  appended log would answer today's question with lines from Tuesday — the same failure, one step
  removed.
- **No ANSI.** The file exists to be grepped and pasted; the console reads fine without colour.

*Related, and the reason this is its own entry rather than a footnote:* the empty grep was treated as
a measurement. §203 already recorded the cost of reasoning past what can be observed, and this is the
inverse mistake — believing an observation without checking the instrument was on. A grep that finds
nothing has two explanations and only one of them is about the code.

*Fifty-ninth: before a null result becomes evidence, prove the instrument was recording. "No hit"
and "no data" print identically.*

## 207. The poll that reported nothing (2026-08-17)

> *"If I don't ask about the status the app is not checking the success or failure. If I asked, the
> success appears even when the agent already finished his work."*

Three rounds went into this without a single measurement, which is the part worth recording.

**What is now established, from evidence rather than reading:**

- *The poll succeeds.* The panel showed `running · data cleaning` and `running · exploratory data
  analysis` — two different activity strings for two workers. `decode_async_tasks` sets
  `activity: None` and `track_task` never copies it from a later snapshot, so an activity string on a
  tracked task can **only** have come from `watch_task`'s own HTTP poll. That rules out a dead
  watcher, which was the leading theory.
- *`next` is present and is an array.* §204's warning fires when it is not, the app now keeps its own
  log (§206), and the log from a real run carries exactly one `WARN` — the host-execution banner.

Which leaves one conclusion: `next` is present and **non-empty** on a worker whose run has ended, so
`next_is_empty` is the wrong test for "done". The coordinator's `check_async_task` reads the
middleware's own record and says `success`, which is why asking works.

And the two are not simply disagreeing about a bug — a turn in this very session had the coordinator
report *"the folders `eda/`, `diagnostic/`, `reports/` and `scripts/` exist but are empty, so the
background task only partially completed despite reporting success."* The middleware records that the
run returned; the thread's checkpoint says the graph never reached its end. Both are telling the
truth about different things, and the panel has been showing the more pessimistic one with no way to
say why.

**What this entry actually changes** is only the instrumentation, deliberately:

- The watcher logs the state its decision is read from, **once per task** — status, `next`, activity.
  Once, not per four-second poll, because a log that scrolls is a log nobody reads.
- It logs again on every change, which is naturally rate-limited: the watcher only forwards changes.
- The poll's failure path was `debug!`, below the default filter. A poll failing every four seconds
  left the panel saying `running` for ever while saying nothing a researcher could see —
  indistinguishable from a worker still working. Now `warn!`, once per task.

The fix follows the value. Three guesses have already been spent, and §203 and §206 both say the same
thing from opposite directions: do not reason past what is observed, and do not accept an observation
until the instrument is known to be recording.

*Sixtieth: a poll that cannot say what it saw is not an observation, it is a hope. Instrument the
decision, not the outcome.*

### 207a. What the instrument said (2026-08-17)

One run, and the poll turned out to be right all along:

```
15:43:11  status=running      next=[model]                              activity=write todos
15:43:15  status=running      next=[model]                              activity=ls
15:43:45  status=interrupted  next=[HumanInTheLoopMiddleware.after_model] activity=execute
15:44:15  status=running      next=[tools]                              activity=execute
15:44:28  status=interrupted  next=[HumanInTheLoopMiddleware.after_model] activity=execute
15:44:32  status=running      next=[tools]                              activity=execute
15:44:41  status=success      next=[]                                   activity=write todos
```

`next=[]` and `status=success`, ninety seconds in, with nobody asking. **The watcher works, `next` is
the right test, and every theory in §204 and §207 was wrong** — including the one that survived two
eliminations.

The gate rounds are worth noticing too: `interrupted` twice with `next=[HumanInTheLoopMiddleware.after_model]`,
each cleared within seconds by the standing approval. That path is doing exactly what §31 built it for.

**So what was reported?** Two different things wearing one sentence.

1. *The waits were real.* The earlier runs generated 500 records and ran a full EDA across two
   workers; four minutes of that is not a stuck panel.
2. *And in that run the middleware was the optimistic one, not the app.* The coordinator itself said
   so: *"the folders `eda/`, `diagnostic/`, `reports/` and `scripts/` exist but are empty — so the
   background task only partially completed despite reporting success."* `check_async_task` read the
   middleware's record and said `success`; the thread's `next` said there was more to do. Asking did
   not reveal a status the app had missed — **it produced a more flattering one.** The panel was
   right and had no way to say why.

That last point is the finding, and it is the opposite of the bug report. It goes on the open list
against the backend, beside §35's theorizer reporting a guess instead of the command's real output —
same shape, same cost: a status a researcher believes because nothing contradicts it.

*Sixty-first: when the instrument finally speaks and contradicts every theory including your own
favourite, that is the instrument working. Three rounds of guessing were the price of not having
built it first.*

## 208. The heading cut off the thing it was added to say (2026-08-17)

A screenshot of §201 working showed the group heading as `...d worker / outputs / tables`.

The full label is `background worker / outputs / tables` — 35 characters into a 28-character budget,
shortened by `distinguishing_tail`, which keeps the **tail**. §152 chose that end deliberately and was
right at the time: its labels shared a long *prefix* (`<uuid>/eda/plots`, `<uuid>/eda/tables`) and
differed at the end, so keeping the tail kept the information. §201 then put the producing worker's
name at the *head* and inverted the assumption without re-checking it — so the one word the whole
feature exists to show became the one word guaranteed to be dropped.

Both ends now survive and the middle gives way: `background worker / … / tables`. If it still will not
fit, the **head** is kept whole and the tail is trimmed, because a heading that cannot say who made
these files is the heading §201 replaced.

The budget was wrong too, and only by two characters — which is why it mattered. The heading box is
`GRID_TILE_COMPACT × GRID_COLUMNS + GRID_GAP` = 304px less about a hundred for `click to open all`,
so roughly 32 characters, not 28. `ui::Label` here carries no `.ellipsis()` (§193 removed it), so that
number is the *only* thing keeping the text inside a fixed-width box — which is now said at the
constant, and there is a test asserting the output fits every budget from 6 to 48.

*Fifty-second… no: sixty-second.* **When you move information to a new position, re-check every rule
that was written for the old one.** The truncation was not a bug when it was written; it became one
the moment something else claimed the head.

### And the background-task hunt closes

Both cases now reach `status=success next=[]` unaided, measured end to end:

| | light run | heavy run |
| --- | --- | --- |
| duration | 90s | 6m 42s |
| approval gate rounds | 2 | 8 |
| seconds per `execute` | ~35 | 35–43 |
| flipped to ✓ without being asked | yes | yes |

So §204's "big bug" was not a bug in the watcher. It was a long run, plus — in the original
observation — a coordinator that reported `success` for work whose folders it then found empty. The
app was showing the honest status and had no way to say so.

What remains from it is real but different, and belongs to the deferred loading-state item: **six
minutes of `running · execute` says nothing about how much is left.** The worker is calling
`write todos` between commands, so a plan exists on its thread. A progress display that reads that
plan would be worth more than any animation.

## 209. Six minutes of "running", with a denominator (2026-08-17)

> *"When we have these long runs, we need a status bar or something to communicate to the user the
> status of the plan or work. I know that deepagents planifies a plan and then executes."*

§208's trace is the argument: a heavy run took **6m 42s** across eight approval rounds, with the
panel saying `running · execute` throughout. §42 had already fixed the worse version of this — the
panel used to say only `running`, and naming the tool was a real gain — but a tool name still cannot
answer *how much is left*.

**The plan is real and was already on the wire.** `TodoListMiddleware` gives every agent a
`write_todos` tool and keeps the result in state as `todos: list[{content, status}]`, with `status`
one of `pending` / `in_progress` / `completed`. §207's own log shows `activity=write todos` between
commands, so the worker maintains it live. Two feeds, both already open:

- **foreground turns** — `todos` sits beside `artifacts` in the `values` frame we already decode. It
  is agent state, not an artifact, which is why it is read from the top level.
- **background workers** — the same `GET /threads/{id}/state` the watcher has been polling for its
  status all along.

Each agent's list is its own: `deepagents`' `_EXCLUDED_STATE_KEYS` keeps `todos` out of what a
subagent inherits. That is what lets a plan belong to exactly one row on screen instead of being a
pile nobody owns.

**What is drawn.** The plan, as a checklist, under the row it belongs to:

```
◐ background worker                                  2 of 4
   ✓  Generate the synthetic guinea pig dataset
   ✓  Clean and validate the columns
   ◐  Build the diagnostic model              execute
   ○  Write the report
```

and one line in the status bar for whoever does not have the panel open —
`background worker · step 3 of 4 · execute`.

**Four rules, each of which something in this project already paid for:**

- **No percentage, no bar, no estimate.** §73's rule — a provenance record that guesses gets
  believed — applies to progress at least as hard. The only honest denominator is the one the agent
  wrote down itself.
- **No plan, no section.** `write_todos` is optional and the model skips it for simple requests, so
  a plan is a thing that sometimes exists. §178's version of this mistake was an invitation to start
  displayed over a conversation already chosen.
- **`done + 1` is the step being worked on.** Two of four finished means the third is running.
  "Step 2 of 4" would be a wrong statement about where the work is, and it is a one-character bug.
- **Replaced, not merged.** A plan is a whole statement of current intent: the model rewrites the
  list to reorder or drop a step, so keeping old items when a shorter list arrives would show work
  the agent has abandoned. Guarded on non-empty for the opposite reason — a frame carrying no
  `todos` is *silent* about the plan, not a claim there is none.

The status line prefers an **unfinished worker** over the conversation's own plan, because a worker
is the thing running while nobody is looking; a finished one says nothing at all, since its row
already carries a tick and a button and a stale count would outlive the work. The rule is a free
function over `(&[AsyncTask], &[Todo])`, tested without a window — §203 and §205 each cost a round
trip to a machine with a screen to learn that.

**Deliberately not in this version:** elapsed time per step. It needs a first-seen stamp per todo
that nothing currently keeps, and the count is the information that was missing. Worth adding when
the plan display has been lived with.

*Sixty-third: "it is working" is not a status. The agent had written down what it was doing all
along; nobody had read it.*

## 211. A worker billed to an account nobody chose (2026-08-17)

> *"I saw that after re-stating an api message even when I can chat with openrouter."*

The panel showed a background worker with the ✗ mark and this beneath it:

```
RateLimitError("Error code: 429 - {'error': {'message': 'You have no credits remaining.
Add credits to continue using the API at https://platform.openai.com/…/billing',
'type': 'insufficient_quota', 'code': 'credit_balance_exhausted'}}")
```

An **OpenAI** billing page, for a researcher whose coordinator is on OpenRouter and whose
conversation chats perfectly well. That is §186 and §187 for the third time, one layer further
down: the coordinator's own turns carry the model, the key and the `base_url` in the request, and a
background worker is a *different run* that only has what was forwarded to it.

Two ways it can go wrong, and they look identical from the panel:

- **The keys did not travel.** `_forwarded_config` already warns when `model_config` is missing —
  the loud case, no model at all. `__llm_keys` had no such line, and it is the quiet one: the worker
  builds exactly the model the researcher chose, has no key or **no `base_url`** for it, and the
  client library falls back to its own default host. For OpenRouter that host is `api.openai.com`.
- **A specialist names a provider no key covers.** §187 already recorded this as open —
  *"nothing stops a specialist being pointed at an unkeyed provider; the gate reads the
  coordinator's settings alone"* — and this is what it looks like when it fires: a 429, minutes
  later, inside a worker, from an account the researcher has never used.

The second is knowable **before the run starts**. `model_config` carries `default` and `subagents`,
every value is a `provider::model_id`, and `__llm_keys` is right there. Comparing the two sets is
four lines, and it turns "the async subagent encountered an error" into the provider's name.

So the overlay now says, at the moment it hands work to a worker:

```
minime_local: background work will bill custom, model custom::moonshotai/kimi-k2
minime_local: background work names openai and carries no key for it — those requests will
  fail inside the worker, on whatever host the client library defaults to
```

**Provider names, the model spec, and whether a `base_url` came with each — never the key.** That is
exactly enough to tell "went to the wrong host" from "had no key at all", and nothing a log should
hold. The org's own rule about credentials is not a thing to be clever around.

This entry is instrumentation, not a fix, and deliberately so: which of the two cases the researcher
hit cannot be settled from a machine with no backend on it, and §203, §206 and §207 each cost a round
trip to relearn that. The fix follows the line.

*Sixty-fifth: a config that travels between runs needs a receipt at both ends. The sender knowing
what it sent is not the same as the receiver knowing what it got.*
## 212. The same model, under two companies (2026-08-17)

§211 instrumented the failure; a sentence from the researcher identified it:

> *"But I have gpt5.4 available from openrouter so why I cannot use it?"*

They could. They had picked it from the wrong company.

`subagent_model_list` lists **every** provider's catalogue under its own heading, which §191 built
deliberately — pointing literature search at a cheap long-context model from another provider is the
main reason the feature exists. What that produces for a researcher on OpenRouter is the same model
twice:

| heading | id | works? |
| --- | --- | --- |
| OpenAI | `gpt-4.1` | needs an OpenAI key |
| OpenRouter | `openai/gpt-4.1` | covered by the key they have |

They differ by a **prefix**. Pick the first and the override saves without complaint, the
conversation keeps chatting because the coordinator's own key is fine, and the first anyone hears of
it is a 429 from an OpenAI billing page, raised inside a background worker several minutes later.

There *was* a `— no key stored` note beside the heading. It was scrolled past, and it would be
scrolled past by every researcher who ever uses this panel. A warning next to four hundred pickable
rows is not a gate; it is a caption.

**Two layers now, because either alone leaves a hole.**

- **The picker stops offering them.** A provider with no key contributes its heading and one line —
  *"412 models here once an OpenAI key is stored — add one under API key above"* — and nothing to
  press. The heading stays: hiding the provider entirely would leave someone who *does* have an
  OpenAI account with no clue why it is not offered.
- **The turn gate reads the specialists too.** §186 refused a turn whose *coordinator* had no key
  and looked no further, which is the whole reason this could happen. Now an override to an unkeyed
  provider is refused in the same place, before anything is spent, naming the specialist and the
  company. This is what catches a settings file written before today, which no picker change can.

*What this cost:* four rounds — §186, §187, §211, and this — for one shape of mistake. Each time the
gate was made stricter about the thing that had just failed, and each time it was still reading one
level of the configuration while the model read all of it.

*Sixty-sixth: a UI that lists everything and annotates the unusable has not warned anyone. If it
cannot be paid for, it should not be pressable.*

## 213. Shipping it to somebody else (2026-08-17)

> *"Let's ship v0.1.0 so we can test with another person."*

The draft release was built on **2026-08-05**, and there are **183 commits** behind it. Publishing
it would have sent a second researcher an app without the wrapping composer, the editable mission,
file authorship, the plan display, or the unkeyed-provider gate — and every bug report would have
been about defects fixed a fortnight earlier. So this is `v0.2.0`, cut from today.

### The app never said which build it was

`CARGO_PKG_VERSION` appeared nowhere: not in the window, not in the log, not in the About page. The
app has logged the *backend checkout's* commit as its very first line since §115 — the entry that
argued a diagnosis without a version costs a night — and said nothing at all about itself.

That is survivable with one user who is also the developer. It is not survivable with a second
person on another machine, where *"it doesn't work"* could be any of 183 commits and the first
question back is always the same one.

So: `Mini-Me Desktop 0.2.0 (4f1e697)` in the first log line and under a **THIS BUILD** heading in
About, selectable because the whole point is pasting it into a message. The commit is stamped by the
release workflow; a local `cargo run` says *"built from source"* rather than implying a release it
is not.

### What the release notes had to gain

They described the install and stopped. Three things a remote tester needs and could not have known:

- **The first run installs WSL, wants administrator rights, and needs a restart.** §61 proved that
  flow works on a third laptop. Nobody told the person who would meet it.
- **Where the two logs are.** §206 gave the app one at last; a tester who cannot find it is back to
  describing symptoms in prose.
- **What to send back** — the build line and both logs, with the assurance that neither holds an API
  key, because the org's rule about credentials is exactly what a researcher will worry about before
  sending a file.

### Still unsigned

SmartScreen will say *"Windows protected your PC"*, and the notes say which two words to click. That
is an organisational decision about a certificate, not an engineering one, and it stays open. Worth
knowing what it costs: for a non-developer audience, that dialog is where most installs end.

*Sixty-seventh: the version is not metadata, it is the first question of every support conversation.
An app that cannot answer it makes every reporter answer it for you, wrongly.*

## 214. Green here, red on the machine that ships it (2026-08-17)

`v0.2.0` was tagged and the release build failed — two tests, both in `references.rs`, both passing
on every run this project has ever made and neither ever run on Windows before. The release
workflow runs `cargo test --release` on a Windows runner, and these tests were written after
`v0.1.0` was cut, so today was the first time they met it.

**`603�627`.** The overlay formats a citation with an en-dash. Python picks its stdout encoding from
the console code page, which on that runner is cp1252, so U+2013 goes out as the single byte `0x96`
— and reading it back as UTF-8 yields the replacement character. The test then compared
`603�627` with `603–627` and failed on a citation the overlay had formatted perfectly.

Nothing about the product: in a real run this text crosses HTTP as JSON, which is UTF-8 by
definition, and the overlay only ever executes inside WSL. `PYTHONIOENCODING=utf-8` on the child,
at all fifteen spawn sites — seven of which pass today only because their Python happens to print
ASCII, and this repository deals in accented Spanish and potato cultivar names.

**`\w\proj\A\B`.** A layout assertion compared `str(pathlib.Path(...))` against `"/w/proj/A/B"`.
The rule under test is *which folder nests inside which*; the separator is the platform's opinion.
`as_posix()` says the rule and not the opinion.

**And one I introduced fixing them.** The first version handed each test a prepared `Command`. A
`Command` accumulates its arguments, and two of these tests spawn inside a loop — so the second
iteration reran the first record's arguments against the second record's expectation and failed
while pointing at the wrong data entirely. Caught locally, and worth recording because the symptom
was so misleading: the assertion named a record that was never the one being run.

The helper now hands out the **program name**, and each spawn builds its own interpreter. It also
tries `python`, `py` and `python3` in turn, because `python3` is not the name on Windows and a test
that skips itself is worse than one that fails: it is green.

*Sixty-eighth: a test suite that has only ever run on one platform is a test suite for that
platform. The release build is where that bill arrives, which is the worst moment to receive it.*
