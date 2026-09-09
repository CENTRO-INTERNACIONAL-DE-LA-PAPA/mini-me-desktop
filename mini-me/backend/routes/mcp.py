"""Availability of optional hosted MCP integrations."""

from __future__ import annotations

from starlette.requests import Request
from starlette.responses import JSONResponse, Response

from backend.mcp_tools import mcp_status_report


async def get_mcp_status(_request: Request) -> Response:
    """Report handshakes performed by the graph factory, never credential material."""
    return JSONResponse(mcp_status_report())
