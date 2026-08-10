# Reports owed upstream

Six defects found while building the desktop client, none of them this repo's to fix. Each file
below is written to be filed as-is: evidence with `file:line`, what it costs, and a suggested fix.

They are kept here rather than filed straight away because filing is an outward-facing act on
somebody else's repository, and the decision of when and by whom is the maintainer's — see
`docs/desktop-app-plan.md` §94.

## Mini-Me (`backend/`, CIP)

| | severity | one line |
|---|---|---|
| [theorizer-reports-a-guess.md](mini-me/theorizer-reports-a-guess.md) | **high** | A tool reports a *cause* it inferred instead of the command's real output; cost seven debugging rounds |
| [guardrails-claims-isolation.md](mini-me/guardrails-claims-isolation.md) | **high** | The approval prompt tells the researcher a command is sandboxed when it may be running on their machine |
| [dev-store-docstring.md](mini-me/dev-store-docstring.md) | low | `make_backend` says the dev store loses content on restart. It does not, and the app tells people to restart |
| [start-async-task-config.md](mini-me/start-async-task-config.md) | **high** | `deepagents` launches background runs with no config, so they inherit no model, key or recursion limit |
| [project-spine-is-not-per-project.md](mini-me/project-spine-is-not-per-project.md) | medium | The research spine is keyed per *user*, so it mixes every study a person has run and never forgets a deleted conversation |
| [academic-sources-drop-the-corpus-id.md](mini-me/academic-sources-drop-the-corpus-id.md) | **high** | Asta's paper search returns only a `corpusId`; it is dropped, and the model is asked for an APA citation it has no data for — so it invents the DOI |
| [subagent-skills-point-one-level-too-deep.md](mini-me/subagent-skills-point-one-level-too-deep.md) | **high** | All ten subagents declare `/skills/<name>/` where the loader wants the parent, so each appears to receive none of the guidance written for it |

## langgraph (`langgraph_runtime_inmem`, LangChain)

Both are silent data loss, and both affect anyone running `langgraph dev`.

| | severity | one line |
|---|---|---|
| [checkpoint-load-failure-overwrites.md](langgraph/checkpoint-load-failure-overwrites.md) | **critical** | A failed checkpoint load leaves an empty dict registered with the flush loop, which writes it over the real file ten seconds later |
| [ops-index-deleted-on-any-error.md](langgraph/ops-index-deleted-on-any-error.md) | **critical** | The thread index is `os.remove`d on any load exception, including ones caused by a missing environment variable |
