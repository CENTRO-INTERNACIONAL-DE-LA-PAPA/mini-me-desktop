"""Stop `execute` from being told to write outside the conversation.

# The defect

A synchronous EDA produced sixteen real files — a 46 KB dataset, seven summary CSVs, seven
figures — and the researcher's Outputs panel was empty. The files were in WSL's global `/tmp`,
where `workspace::outputs` cannot see them, artifact capture correctly does not scan, a project
move does not carry them, deleting the conversation does not remove them, and the operating system
eventually deletes them anyway (docs §160).

The coordinator had asked for relative paths. It lost, and to a better-placed instruction:

    deepagents/middleware/filesystem.py:422
    "Try to maintain your current working directory throughout the session by using absolute
     paths and avoiding usage of cd"

That is sound advice for an agent in a container it owns. Here it is the opposite of what is
needed: `LocalWorkspaceBackend` runs with ``virtual_mode=False`` so that shell commands and file
tools share one real path namespace (§18), which means an absolute path in a command is an
absolute path on the researcher's machine. `/tmp` is a plausible guess for a scratch directory,
and nothing corrected it.

# Why the description and not a guard

`_reroute_write` re-roots absolute paths for `write` and `upload_files`. `aexecute` has no
equivalent and should not get one made of string matching: a command is an arbitrary shell
program, and pattern-matching it for writes would produce a containment claim that is false in
every case nobody thought of. §160 says this and then proposes exactly that as a fallback; it
cannot be both.

So this changes what the model is *told*, which is honest about being advice rather than a
boundary. Real containment means the workspace is the only writable persistent mount, and that is
a larger change than a docstring — one worth making, and not one to pretend has been made.

# Why it is patched by name

`create_deep_agent` takes no `custom_tool_descriptions`, and `FilesystemMiddleware` reads the
module global when it builds the tool (`filesystem.py:1481`), so replacing the global before any
middleware is constructed is the reachable point. Patching a third party by name is a shape this
project has been bitten by, so: the replacement is attempted on an exact sentence and the result
is **logged either way**. If upstream rewords that line, the log says the contradiction may have
returned rather than reporting success over a no-op.
"""

from __future__ import annotations

import logging

log = logging.getLogger(__name__)

#: The sentence to remove, quoted exactly from the pinned deepagents.
_ABSOLUTE_ADVICE = (
    "Try to maintain your current working directory throughout the session by using absolute "
    "paths and avoiding usage of cd"
)

#: What replaces it. Same shape — one line, same place — so the surrounding guidance about `&&`
#: versus `;` and about timeouts is left exactly as upstream wrote it.
_RELATIVE_ADVICE = (
    "Stay in your current working directory: it is already this conversation's workspace. Use "
    "relative paths and avoid usage of cd"
)

#: Appended regardless, because the replacement above can silently fail to apply and this is the
#: part that names the consequence. Written for the model in the second person, and concrete:
#: "outside the workspace" means nothing to it, "will not appear in Outputs" does.
_WORKSPACE_RULE = """

Where your output goes:
  - Your working directory IS this conversation's folder. Write files there, with relative paths
    like `results/summary.csv` or `figure.png`.
  - Do NOT write results to `/tmp`, `/data`, `/plots`, your home directory, or any other absolute
    path. Those are outside this conversation. Files written there do not appear in the
    researcher's Outputs panel, are not kept when the conversation is filed or deleted, and are
    erased by the operating system without warning. The work is done and then lost.
  - Reading an absolute path is fine — a researcher may attach a dataset from anywhere. It is
    only persistent *output* that belongs in the working directory.
"""


def install(module) -> None:
    """Rewrite `EXECUTE_TOOL_DESCRIPTION` on deepagents' filesystem middleware module."""
    original = getattr(module, "EXECUTE_TOOL_DESCRIPTION", None)
    if not isinstance(original, str) or not original:
        log.warning(
            "minime_local: deepagents has no EXECUTE_TOOL_DESCRIPTION to rewrite — commands "
            "will be told to prefer absolute paths and their outputs may leave the workspace "
            "(docs §160)"
        )
        return

    replaced = original.replace(_ABSOLUTE_ADVICE, _RELATIVE_ADVICE)
    if replaced == original:
        # Not fatal, and not silent. The appended rule still ships; what is lost is the removal
        # of the sentence arguing against it, and a reader deserves to know which of the two
        # they got.
        log.warning(
            "minime_local: deepagents' execute description no longer contains the "
            "absolute-path sentence — the workspace rule is appended, but the advice it "
            "contradicts may have returned in a new wording (docs §160)"
        )
    module.EXECUTE_TOOL_DESCRIPTION = replaced + _WORKSPACE_RULE
    log.warning(
        "minime_local: execute is told to keep persistent output in the conversation's workspace"
    )
