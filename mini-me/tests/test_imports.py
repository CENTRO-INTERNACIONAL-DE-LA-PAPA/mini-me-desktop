"""Smoke tests guarding the LangGraph deploy entry points.

``langgraph.json`` wires the deployment via dotted module paths
(``backend/agent.py:agent``, ``backend/routes/__init__.py:app``, ``backend/auth.py:auth``). If any of
these stop importing cleanly the server will not boot, so these tests are run
after every step of the backend modularization to catch a broken import surface
before it reaches a real ``langgraph dev`` boot.
"""

from __future__ import annotations


def test_agent_entrypoint_importable() -> None:
    from backend.agent import agent

    assert callable(agent), "backend.agent:agent must be a callable graph factory"


def test_http_app_entrypoint_importable() -> None:
    from backend.routes import app

    assert app is not None, "backend.routes:app must be defined"


def test_auth_entrypoint_importable() -> None:
    from backend.auth import auth

    assert auth is not None, "backend.auth:auth must be defined"
