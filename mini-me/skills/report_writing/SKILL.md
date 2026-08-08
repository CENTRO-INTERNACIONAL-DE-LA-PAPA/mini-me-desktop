---
name: report_writing
description: >-
  Synthesize outputs from research, data cleaning, EDA, diagnostic analytics,
  and predictive analytics into a coherent markdown report that preserves
  uncertainty, caveats, and evidence boundaries. Use when the task is to write
  a final report, executive summary, technical memo, or decision-ready markdown
  narrative from prior analysis results.
---

# Report Writing Guidelines

Use this skill when the goal is to produce a final **markdown-only** report
from existing analysis outputs.

This skill is for synthesis and communication. It does not perform new
analysis.

## Goals

- Turn prior outputs into a clear markdown report.
- Preserve the meaning, uncertainty, and caveats of upstream results.
- Separate descriptive, diagnostic, and predictive claims clearly.
- Adapt the report to the audience without inventing evidence.

## Boundaries

This skill does **not** own:

- data cleaning
- exploration
- inference
- prediction
- new literature review

Do not rerun analyses unless the user explicitly asks for that and another
subagent should handle it first.

## Core rules

- Write only in markdown.
- Do not invent results, metrics, references, plots, or conclusions.
- If an upstream output is missing, say it is missing.
- If upstream analyses disagree, surface the disagreement explicitly.
- Preserve uncertainty language from diagnostic and predictive outputs.
- Keep methods concise in the main text unless the audience is highly technical.
- Put technical detail, reproducibility notes, and code-related material in an Appendix.

## Inputs to expect

This skill may need to synthesize any combination of:

- Academic Research outputs
- Dataverse findings
- Data Cleaning findings
- EDA findings
- Diagnostic Analytics findings
- Predictive Analytics findings
- user constraints about audience, tone, and length

## Report modes

Use [references/report_structures.md](references/report_structures.md) to pick
the structure.

Supported modes:

- `scientific_report`
- `executive_summary`
- `technical_memo`

Default to `scientific_report` unless the user asks for a shorter or more
decision-focused output.

## Style and tone

Read [references/style_and_tone.md](references/style_and_tone.md) before
writing.

Use:

- precise language
- short paragraphs
- explicit caveats
- section headings that make the report easy to scan

Avoid:

- hype
- vague claims
- unsupported certainty
- dumping raw intermediate outputs into the main narrative

## Evidence rules

Read [references/evidence_rules.md](references/evidence_rules.md) before
finalizing the report.

Keep these boundaries explicit:

- EDA describes what happened or what patterns appear
- Diagnostic Analytics discusses plausible explanations or associations
- Predictive Analytics discusses future outcomes and expected performance
- Data Cleaning describes what was fixed, what remained problematic, and how
  that affects trust in later findings

## Preferred markdown structure

For a default scientific-style report, use this shape:

1. Title
2. Objective
3. Data and Scope
4. Methods Overview
5. Key Findings
6. Limitations and Caveats
7. Recommendations and Next Steps
8. References
9. Appendix

Within `Key Findings`, separate findings by source when helpful:

- Data quality and cleaning findings
- Exploratory findings
- Diagnostic findings
- Predictive findings

## Template use

Use the markdown templates in `assets/` as starting points when helpful:

- `assets/scientific_report_template.md`
- `assets/executive_summary_template.md`
- `assets/technical_memo_template.md`

Adapt them to the actual available evidence. Do not force empty sections if the
evidence does not exist.

## Appendix rules

Use the Appendix for:

- model details
- metric tables
- code notes or full code blocks
- reproducibility notes
- extended caveats

If the user explicitly asks for code:

- include full code in fenced markdown blocks such as ` ```python `
- the code does not need to be packaged as a standalone executable file
- the code should faithfully represent the analytical workflow, not just a vague sketch

If the user does not ask for code:

- include concise code notes, script names, or file references only when useful

Do not bury critical decision-relevant limitations in the Appendix only.

## Expected output

Return one coherent markdown document that:

- answers the user's reporting need
- clearly distinguishes evidence types
- preserves caveats
- includes actionable next steps when appropriate
