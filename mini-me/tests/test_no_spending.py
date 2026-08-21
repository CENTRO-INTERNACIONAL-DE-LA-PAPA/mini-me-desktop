"""Guards on the one thing in this app that spends a researcher's money.

`autodiscovery_tools` claimed "no code path from a model decision to a spent credit". A review found
that false: the *tool* surface has no submit, but `execute` is a general shell every agent keeps,
`ASTA_TOKEN` is injected into every command it runs, and `asta autodiscovery submit <id> -y` is a
shell command. With `approve_execute = false` — a supported setting — the whole grant was one
instruction away.

These tests pin the guard that replaced the claim. Docs §252.
"""

from __future__ import annotations

from types import SimpleNamespace

import asyncio

from backend.middleware.no_spending import (
    NoSpendingWithoutApproval,
    REFUSAL,
    spends_credits,
)


def _request(name: str, **args):
    return SimpleNamespace(tool_call={"name": name, "id": "call-1", "args": args})


def test_the_credit_spending_commands_are_refused():
    """The shape a model would actually produce, told to do it in a prompt."""
    for command in (
        "asta autodiscovery submit abc-123 -y",
        "asta autodiscovery submit abc-123",
        # Formatting must not defeat it.
        "asta   autodiscovery\n  submit abc-123 -y",
        "ASTA AUTODISCOVERY SUBMIT abc-123",
        # A fork copies the parent's budget and is submittable: the same decision one step removed.
        "asta autodiscovery fork abc-123",
        # And the app's own gate, reached over HTTP instead of through the CLI.
        "curl -X POST http://127.0.0.1:2024/discovery/t/r/submit",
        "python3 -c \"import urllib.request; urllib.request.urlopen('http://x/discovery/t/r/submit')\"",
    ):
        assert spends_credits(command), command


def test_reading_a_run_is_not_spending():
    """The guard must not block the tool doing its job, or it gets removed."""
    for command in (
        "asta autodiscovery create",
        "asta autodiscovery upload abc /workspace/soc.csv",
        "asta autodiscovery metadata abc --file /workspace/.staged.json",
        "asta autodiscovery status abc --format json",
        "asta autodiscovery experiments abc --format json",
        "asta autodiscovery credits --format json",
        # A different service's submit is a different budget, and not this guard's business.
        'asta analyze-data submit "does rainfall predict yield?" data.csv',
        "python3 -c 'import pandas as pd; print(pd.read_csv(\"soc.csv\").shape)'",
    ):
        assert not spends_credits(command), command


def test_the_refusal_reaches_the_model_instead_of_the_shell():
    ran: list[str] = []

    def handler(request):
        ran.append(request.tool_call["args"].get("command", ""))
        return SimpleNamespace(content="ok")

    guard = NoSpendingWithoutApproval()
    blocked = guard.wrap_tool_call(
        _request("execute", command="asta autodiscovery submit abc -y"), handler
    )
    assert ran == [], "the command must never reach the shell"
    assert blocked.status == "error"
    assert "spend the researcher" in blocked.content
    assert "draft_discovery_run" in blocked.content, "a refusal has to say what is allowed"
    assert blocked.tool_call_id == "call-1"

    allowed = guard.wrap_tool_call(
        _request("execute", command="asta autodiscovery status abc"), handler
    )
    assert ran == ["asta autodiscovery status abc"]
    assert allowed.content == "ok"


def test_the_guard_reads_every_command_shaped_argument():
    """A sibling shell tool naming its argument `cmd` or `script` must not slip past."""
    guard = NoSpendingWithoutApproval()

    def handler(request):
        raise AssertionError("should not have run")

    for key in ("command", "cmd", "script", "code", "input"):
        blocked = guard.wrap_tool_call(
            _request("some_shell", **{key: "asta autodiscovery submit abc -y"}), handler
        )
        assert blocked.content == REFUSAL, key


def test_the_guard_watches_tools_other_than_execute():
    """It checks the command, not the tool's name — a second shell under a new name is the change
    most likely to reopen this quietly."""
    guard = NoSpendingWithoutApproval()

    def handler(request):
        raise AssertionError("should not have run")

    blocked = guard.wrap_tool_call(
        _request("run_shell_v2", command="asta autodiscovery submit abc -y"), handler
    )
    assert blocked.status == "error"


def test_the_async_path_refuses_too():
    """`awrap_tool_call` is the one that actually runs; the sync one exists for completeness."""
    guard = NoSpendingWithoutApproval()

    async def handler(request):
        raise AssertionError("should not have run")

    blocked = asyncio.run(
        guard.awrap_tool_call(
            _request("execute", command="asta autodiscovery submit abc -y"), handler
        )
    )
    assert blocked.status == "error"


def test_the_guard_is_attached_to_the_coordinator_and_every_subagent():
    """Credits are the same credits whichever agent spends them."""
    from backend.middleware.guardrails import _build_guardrail_middleware
    from backend.subagents import _build_runtime_subagents

    assert any(
        isinstance(m, NoSpendingWithoutApproval) for m in _build_guardrail_middleware([])
    ), "the coordinator"

    class Resolver:
        def for_subagent(self, name, overrides):
            return "openai::gpt-4o-mini"

    class Sandbox:
        async def aget_work_dir(self):
            return "/w"

    built = _build_runtime_subagents(
        academic_research_tools=[],
        dataverse_tools=[],
        data_cleaning_tools=[],
        diagnostic_tools=[],
        theory_tools=[],
        datavoyager_tools=[],
        discovery_tools=[],
        file_sync=object(),
        sandbox_backend=Sandbox(),
        model_resolver=Resolver(),
        subagent_overrides={},
    )
    assert built, "there are subagents to check"
    for subagent in built:
        assert any(
            isinstance(m, NoSpendingWithoutApproval) for m in subagent["middleware"]
        ), subagent["name"]


def test_every_middleware_this_project_adds_can_actually_be_registered():
    """§253: the test that was missing, and the reason the app would not open.

    `NoSpendingWithoutApproval` was duck-typed — `wrap_tool_call` and nothing else. Every unit test
    above passed, because they call the middleware directly. But `create_agent` reads its hooks off
    the *class*, so building the graph raised

        AttributeError: type object 'NoSpendingWithoutApproval' has no attribute 'before_agent'

    and every conversation hung on "Opening this conversation…". A guard that stops the app from
    opening is worse than the hole it closes.

    So this asserts the property that matters and that a direct call cannot: the middleware is a
    real `AgentMiddleware` and a graph carrying it builds.
    """
    from langchain.agents.middleware import AgentMiddleware

    assert isinstance(NoSpendingWithoutApproval(), AgentMiddleware)


def test_a_graph_carrying_the_guard_builds():
    import os

    from langchain.agents import create_agent
    from langchain_core.tools import tool

    @tool
    def execute(command: str) -> str:
        """Run a shell command."""
        return "ran"

    # `create_agent` resolves a model eagerly; a placeholder is enough, nothing is called.
    os.environ.setdefault("OPENAI_API_KEY", "sk-not-used-by-this-test")
    agent = create_agent(
        model="openai:gpt-4o-mini",
        tools=[execute],
        middleware=[NoSpendingWithoutApproval()],
    )
    assert agent is not None


def test_the_whole_middleware_stack_is_registerable():
    """Not just this one. Every class the guardrail builder returns has to survive graph
    construction, and this is the cheapest way to find out."""
    from langchain.agents.middleware import AgentMiddleware

    from backend.middleware.guardrails import _build_guardrail_middleware

    for middleware in _build_guardrail_middleware(["find_papers"]):
        assert isinstance(middleware, AgentMiddleware), type(middleware).__name__
