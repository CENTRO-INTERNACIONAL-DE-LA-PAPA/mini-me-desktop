"""Shared execution helpers for the deepagents virtual filesystem backend.

This app always runs the agent's code and file I/O on the local host, through
``backend.local.workspace.LocalWorkspaceBackend``. What is left here is the handful of
helpers that backend shares with the rest of the codebase: output truncation and the
`sandbox_status` stream event.
"""

EXECUTE_OUTPUT_MAX_BYTES = 32_000
EXECUTE_OUTPUT_HEAD_BYTES = 16_000
EXECUTE_OUTPUT_TAIL_BYTES = 12_000


def _truncate_execute_response(response):
    """Cap aexecute output so chatty libraries cannot poison agent state.

    PyMC samplers, scikit-learn verbose modes, and other numerical libs
    can emit MB-scale stdout/stderr even with progressbar/verbose flags.
    That output is captured verbatim and returned to the LLM — and then
    persisted in LangGraph state, re-serialized on every stream tick.
    A multi-MB tool message crashes the browser renderer (SIGILL in V8)
    long before ReactMarkdown gets involved.

    We keep head + tail so the model still sees the start (which usually
    indicates what ran) and the end (which usually contains the final
    summary / error), and we mark the response as truncated so the model
    knows the middle was elided.
    """
    output = getattr(response, "output", None)
    if not isinstance(output, str) or len(output.encode("utf-8")) <= EXECUTE_OUTPUT_MAX_BYTES:
        return response

    encoded = output.encode("utf-8")
    head = encoded[:EXECUTE_OUTPUT_HEAD_BYTES].decode("utf-8", errors="ignore")
    tail = encoded[-EXECUTE_OUTPUT_TAIL_BYTES:].decode("utf-8", errors="ignore")
    dropped_kb = (len(encoded) - EXECUTE_OUTPUT_HEAD_BYTES - EXECUTE_OUTPUT_TAIL_BYTES) // 1024
    response.output = (
        f"{head}\n\n"
        f"...[output truncated — {dropped_kb} KB elided to protect agent state; "
        f"redirect verbose library output to a log file in the work dir if you need the full trace]...\n\n"
        f"{tail}"
    )
    response.truncated = True
    return response


def _emit_sandbox_status(state: str, message: str = "") -> None:
    """Emit a custom 'sandbox_status' event to the LangGraph stream.

    Best-effort: when no stream writer is available (outside a graph run,
    e.g. during HTTP-route resolution), this is a no-op. The frontend
    listens on the 'custom' stream-mode channel for these events.

    Args:
        state: one of 'preparing', 'ready', 'error'.
        message: human-readable detail shown in the UI.
    """
    try:
        from langgraph.config import get_stream_writer
    except Exception:
        return
    try:
        writer = get_stream_writer()
    except Exception:
        return
    if writer is None:
        return
    try:
        writer({"sandbox_status": {"state": state, "message": message}})
    except Exception:
        # Never let a status emission failure break agent execution.
        pass


