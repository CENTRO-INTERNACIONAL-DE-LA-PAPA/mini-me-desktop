"""Whose spine a call reads: a project's, one conversation's, or nobody's.

The bug this file exists for was visible on a researcher's screen. A brand-new conversation about
potato late blight opened showing a mission of *"Testting functionalities"*, six visualizations it
had not produced, and the plan from the conversation before it — because every conversation the
researcher had never filed into a project shared one spine record.

The panel was the visible half. `render_mission_context` puts the same spine into the
**coordinator's system prompt** each turn, under *"Ground every answer, plan, and delegation in
this mission"*, so an unrelated conversation began by being told what it had already achieved.
"""

from __future__ import annotations

import asyncio

import pytest

from minime_local import spine


@pytest.fixture
def request_scope():
    """Set and unset the two request variables, so one test cannot leak into the next."""
    tokens: list = []

    def scope(project: str = "", thread: str = ""):
        tokens.append(spine._http_project.set(project))
        tokens.append(spine._http_thread.set(thread))

    yield scope
    for token in reversed(tokens):
        (spine._http_thread if token.var is spine._http_thread else spine._http_project).reset(
            token
        )


def test_a_conversation_names_itself_and_nothing_else():
    assert spine.solo_scope("01a043eb-54b4-7922-8154-c8e5e67861d5") == (
        "solo-01a043eb-54b4-7922-8154-c8e5e67861d5"
    )
    # No conversation is not a conversation named "solo-".
    assert spine.solo_scope("") == ""
    assert spine.solo_scope("   ") == ""


def test_two_unfiled_conversations_do_not_share_a_spine(request_scope):
    """**The defect, stated as a test.** Both are in no project; neither is the other's."""
    request_scope(thread="01a043e1-c08d-7180-84ea-229829e42ab1")
    first = spine.current_scope()
    spine._http_thread.set("01a043eb-54b4-7922-8154-c8e5e67861d5")
    second = spine.current_scope()

    assert first and second
    assert first != second, (
        "a conversation in no project must not read the record another one wrote"
    )


def test_a_project_outranks_the_conversation_inside_it(request_scope):
    """Filing a conversation into a project says its work belongs with that project's (§109)."""
    request_scope(project="Potato Late Blight", thread="01a043eb-54b4-7922-8154-c8e5e67861d5")
    assert spine.current_scope() == "Potato Late Blight"


def test_naming_neither_still_reads_the_shared_record(request_scope):
    """Left reachable on purpose: a client that knows about neither parameter is unchanged."""
    request_scope()
    assert spine.current_scope() == ""


def test_inside_a_run_the_scope_comes_from_the_config(monkeypatch):
    """A turn sets no request variables; it has a run config instead."""
    import langgraph.config

    from minime_local.workspace import WORKSPACE_PROJECT_KEY

    monkeypatch.setattr(
        langgraph.config,
        "get_config",
        lambda: {"configurable": {"thread_id": "01a043eb-54b4-7922", WORKSPACE_PROJECT_KEY: ""}},
    )
    assert spine.current_scope() == "solo-01a043eb-54b4-7922"

    monkeypatch.setattr(
        langgraph.config,
        "get_config",
        lambda: {
            "configurable": {"thread_id": "01a043eb-54b4-7922", WORKSPACE_PROJECT_KEY: "TEST3"}
        },
    )
    assert spine.current_scope() == "TEST3", "the project still wins inside a run"


def test_outside_a_run_and_outside_a_request_nothing_is_claimed(monkeypatch):
    import langgraph.config

    def explode():
        raise RuntimeError("no runnable context")

    monkeypatch.setattr(langgraph.config, "get_config", explode)
    assert spine.current_scope() == ""


def test_the_namespace_gains_the_scope_and_only_the_scope(request_scope):
    """What `install_runtime` actually does to `_project_namespace`, end to end."""

    class FakeRuntime:
        @staticmethod
        def _project_namespace(user_id, project_id):
            return (user_id, "project", project_id)

    spine.install_runtime(FakeRuntime)

    request_scope(thread="01a043eb-54b4")
    assert FakeRuntime._project_namespace("u1", "default") == (
        "u1",
        "project",
        "default",
        "solo-01a043eb-54b4",
    )

    spine._http_thread.set("")
    spine._http_project.set("TEST3")
    assert FakeRuntime._project_namespace("u1", "p5") == ("u1", "project", "p5", "TEST3")


def test_the_route_reads_both_parameters_and_puts_them_back():
    """The wrapper has to be `async` and `await` inside: a sync one resets before the handler runs.

    That mistake is recorded in `spine.py` — it would set the variable, build the coroutine,
    reset, and return it unawaited, so every request read the unscoped spine while looking fine.
    """
    seen: list[str] = []

    class FakeRoutes:
        @staticmethod
        async def get_project(request):
            seen.append(spine.current_scope())
            return "ok"

    spine.install_routes(FakeRoutes)

    class FakeRequest:
        def __init__(self, params):
            self.query_params = params

    ask = FakeRoutes.get_project
    assert asyncio.run(ask(FakeRequest({spine.THREAD_PARAM: "01a043eb"}))) == "ok"
    assert asyncio.run(ask(FakeRequest({spine.QUERY_PARAM: "TEST3"}))) == "ok"
    assert seen == ["solo-01a043eb", "TEST3"]
    # And the variables are back to empty, or the next request inherits this one's scope.
    assert spine._http_project.get() == "" and spine._http_thread.get() == ""
