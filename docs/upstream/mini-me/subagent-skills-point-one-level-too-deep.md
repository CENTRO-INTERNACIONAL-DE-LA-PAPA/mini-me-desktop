# Every subagent declares its skill one directory too deep, so none of them load

**Repo:** Mini-Me (`backend/`)
**Severity:** high — ten subagents appear to receive none of the guidance written for them
**Found:** 2026-08-07
**Confidence:** the path shapes are certain; the loader behaviour was reproduced against a
locally reconstructed store rather than a live deployment. See *What would settle it* below.

## Summary

The skills on disk are laid out one directory per skill, each containing a `SKILL.md`:

```
skills/research/SKILL.md
skills/dataverse/SKILL.md
skills/EDA_subagent/SKILL.md
… twelve in total
```

The coordinator asks for the parent:

```python
skills=["/skills/"]                      # backend/agent.py:138
```

Every subagent asks for its own directory:

```python
"skills": ["/skills/research/"]          # backend/subagents.py:48
"skills": ["/skills/dataverse/"]         # :75
"skills": ["/skills/data_cleaning/"]     # :90
… all ten
```

`deepagents`' loader scans a path's **subdirectories** for a `SKILL.md`
(`deepagents/middleware/skills.py:749-762`). Given `/skills/`, it finds twelve subdirectories and
loads twelve skills. Given `/skills/research/`, it finds no subdirectories — `SKILL.md` is a file —
and loads none. The prompt then renders *"(No skills available yet…)"* (`skills.py:938-939`).

A subagent cannot fall back on the parent's either: `skills_metadata` is explicitly stripped from
the state passed to children (`deepagents/middleware/subagents.py:186-192`).

So the coordinator has all twelve skills, and the ten specialists that each skill was written for
appear to have none.

## Why it is worth chasing

`skills/research/SKILL.md:69-82` names the seven Asta tools by purpose — which one finds a paper by
title, which returns full metadata, which returns text snippets. `academic_researcher` receives all
seven at runtime, unfiltered (`backend/mcp_tools.py:413-414`), and its own prompt mentions none of
them (`backend/subagents.py:36-47`).

That gap has a measured consequence. Asked for *"APA-format references"*, the subagent uses
`snippet_search` — the one tool whose purpose is guessable from its name — which returns a title,
authors and a `corpusId`, and no year, venue, volume, pages or DOI. It then supplies those from
memory. Six references from one run were checked against Crossref: three DOIs resolved to different
real papers, three matched nothing. Written up separately in
`academic-sources-drop-the-corpus-id.md`.

The same shape presumably affects the other nine: each was given a document explaining how to do
its job, and does not receive it.

## The fix

One character per line — the trailing path segment:

```python
"skills": ["/skills/"]        # and let the subagent's own prompt scope which it uses
```

or, if the intent is that each subagent sees only its own, a loader that accepts a path
*containing* a `SKILL.md` as well as one containing directories that do.

The second is probably the better change: `README.md:188-190` states the intent that each
`SKILL.md` be read by its relevant subagent, and loading all twelve everywhere would put eleven
irrelevant documents in each specialist's context.

## What would settle it

This was reproduced by driving `deepagents`' skills loader against a store reconstructed from the
on-disk tree. Nothing in the checkout seeds the LangGraph store from `skills/` — only
`backend/middleware/sync.py:194-198` reads that tree — so a production deployment may populate the
store with a flatter layout in which `/skills/research/` is the correct depth.

The cheap check on a live backend: log `skills_metadata` for one subagent turn, or look for
*"(No skills available yet"* in an assembled subagent prompt. If it is there, this is real.
