"""Custom HTTP routes for AskTheData (wired via langgraph.json: routes.py:app).

Streams artifact files generated inside a thread's LangSmith Sandbox so the
frontend can render inline previews (images) and download arbitrary files,
renders markdown reports to PDF, and serves the model/API-key config endpoints.
The handlers live in sibling modules; this package assembles them into the
Starlette ``app``.
"""

from starlette.applications import Starlette
from starlette.routing import Route

from backend.routes.artifacts import (
    analyze_data_status,
    delete_sandbox,
    get_artifact_file,
    start_sandbox,
    theorizer_status,
    upload_artifact_file,
)
from backend.routes.config import (
    delete_asta_token,
    delete_key,
    get_asta_status,
    get_config,
    save_asta_token,
    save_config,
    save_key,
    test_key,
)
from backend.routes.project import get_project, patch_project
from backend.routes.projects import (
    assign_thread_project_route,
    create_project_route,
    delete_project_route,
    list_projects_route,
    patch_project_meta_route,
)
from backend.routes.rendering import render_report


app = Starlette(
    routes=[
        Route("/files/{thread_id}", endpoint=get_artifact_file, methods=["GET"]),
        Route("/upload/{thread_id}", endpoint=upload_artifact_file, methods=["POST"]),
        Route("/render-report/{thread_id}", endpoint=render_report, methods=["POST"]),
        Route("/sandboxes/{thread_id}", endpoint=delete_sandbox, methods=["DELETE"]),
        Route("/sandboxes/{thread_id}/start", endpoint=start_sandbox, methods=["POST"]),
        Route(
            "/theorizer/{thread_id}/{task_id}",
            endpoint=theorizer_status,
            methods=["GET"],
        ),
        Route(
            "/analyze-data/{thread_id}/{task_id}",
            endpoint=analyze_data_status,
            methods=["GET"],
        ),
        Route("/config", endpoint=get_config, methods=["GET"]),
        Route("/config", endpoint=save_config, methods=["PUT"]),
        Route("/config/keys", endpoint=save_key, methods=["POST"]),
        Route("/config/keys/{provider}", endpoint=delete_key, methods=["DELETE"]),
        Route("/config/test", endpoint=test_key, methods=["POST"]),
        Route("/config/asta", endpoint=get_asta_status, methods=["GET"]),
        Route("/config/asta", endpoint=save_asta_token, methods=["POST"]),
        Route("/config/asta", endpoint=delete_asta_token, methods=["DELETE"]),
        Route("/project", endpoint=get_project, methods=["GET"]),
        Route("/project", endpoint=patch_project, methods=["PATCH"]),
        Route("/projects", endpoint=list_projects_route, methods=["GET"]),
        Route("/projects", endpoint=create_project_route, methods=["POST"]),
        Route("/projects/{project_id}", endpoint=patch_project_meta_route, methods=["PATCH"]),
        Route("/projects/{project_id}", endpoint=delete_project_route, methods=["DELETE"]),
        Route(
            "/threads/{thread_id}/project",
            endpoint=assign_thread_project_route,
            methods=["PUT"],
        ),
    ],
)
