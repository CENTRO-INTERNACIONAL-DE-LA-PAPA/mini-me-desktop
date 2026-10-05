"""Bundled AskPapa Dataverse MCP (Apache-2.0).

Adapted for Mini-Me's FastMCP 4 runtime and cross-platform temporary directories.
See README.md and LICENSE in this package for provenance and licensing.
"""

import asyncio
import json
import os
import re
import uuid
import zipfile
from contextlib import asynccontextmanager
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from tempfile import gettempdir
from typing import Annotated, Any, AsyncIterator, Dict, List, Optional, Union, cast

import aiofiles
import httpx
from dotenv import load_dotenv
from fastmcp import Context, FastMCP
from fastmcp.prompts import Message as PromptMessage
from pydantic import Field

# --- Configuration and Lifespan ---

load_dotenv(os.environ.get("ENV_FILE"))
API_KEY = (os.environ.get("DATAVERSE_API_KEY") or "").strip() or None
BASE_URL = os.environ.get(
    "DATAVERSE_BASE_URL",
    "https://data.cipotato.org/",
)  # https://data.cipotato.org
#
# Installed code can be read-only. Keep runtime files in the OS temporary directory.
_DEFAULT_OUTPUT_ROOT = Path(gettempdir()) / "mcp"
DEFAULT_JSON_DIR = Path(
    os.environ.get("MCP_JSON_DIR", str(_DEFAULT_OUTPUT_ROOT / "json_files"))
)
DEFAULT_DOWNLOAD_DIR = Path(
    os.environ.get("MCP_DOWNLOAD_DIR", str(_DEFAULT_OUTPUT_ROOT / "downloaded_files"))
)
DATASET_TEMPLATE_REGISTRY: Dict[str, Dict[str, Any]] = {
    "finch_minimal": {
        "path": Path(__file__).parent / "dataset-finch1.json",
        "description": "Minimal dataset metadata (Finch example) with required citation fields populated.",
    },
    "all_fields_example": {
        "path": Path(__file__).parent / "dataset-create-new-all-default-fields.json",
        "description": "Full dataset metadata covering default Dataverse fields.",
    },
    "edit_metadata_sample": {
        "path": Path(__file__).parent / "dataset-edit-metadata-sample.json",
        "description": "Sample payload for add/edit metadata calls (subjects, title, producers, etc.).",
    },
    "dataset_schema_example": {
        "path": Path(__file__).parent / "dataset-schema.json",
        "description": "JSON schema for a datasetVersion payload (license + citation metadataBlocks).",
    },
}

# Known top-level Dataverse aliases on CIP Dataverse (used by list_dataverse_collections)
KNOWN_DATAVERSE_ROOT_ALIASES: List[str] = ["cipdata", "dvn"]

# CIP org defaults (used by resource templates)
CIP_ORG_NAME = "International Potato Center - CIP"
CIP_ORG_ABBREV = "CIP"
CIP_ORG_URL = "http://cipotato.org/"
CIP_ORG_LOGO_URL = "https://cipotato.org/site/logo/images/CIP_Logo_12cm_RGB.jpg"

# Last Dataverse HTTP error captured by tools (debug aid).
# Exposed via dataverse://debug/last-error
_LAST_DATAVERSE_HTTP_ERROR: Optional[Dict[str, Any]] = None

_MAX_SAFE_FILENAME_LENGTH = 96


def _set_last_dataverse_http_error(
    *, operation: str, url: str, status_code: int, response_text: str
) -> None:
    global _LAST_DATAVERSE_HTTP_ERROR
    _LAST_DATAVERSE_HTTP_ERROR = {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "operation": operation,
        "url": url,
        "status_code": int(status_code),
        "response_text": response_text,
    }


def _sanitize_output_filename(
    filename: Optional[str],
    *,
    default_stem: str,
    default_suffix: str,
    max_length: int = _MAX_SAFE_FILENAME_LENGTH,
) -> str:
    """Return a short basename that is safe to create under managed output dirs."""
    raw_name = str(filename or "").strip()
    candidate = Path(raw_name).name

    if not candidate:
        candidate = f"{default_stem}{default_suffix}"

    raw_suffix = Path(candidate).suffix
    if (
        not raw_suffix
        or len(raw_suffix) > 16
        or not re.fullmatch(r"\.[A-Za-z0-9]{1,15}", raw_suffix)
    ):
        suffix = default_suffix
    else:
        suffix = raw_suffix

    raw_stem = Path(candidate).stem or default_stem
    stem = re.sub(r"[^A-Za-z0-9._-]+", "_", raw_stem).strip("._-")
    if not stem:
        stem = default_stem

    max_stem_length = max(8, max_length - len(suffix))
    if (
        len(stem) > max_stem_length
        or raw_name != candidate
        or raw_stem != stem
        or suffix != raw_suffix
    ):
        digest = uuid.uuid5(uuid.NAMESPACE_URL, raw_name or candidate).hex[:8]
        truncated = stem[: max_stem_length - 9].rstrip("._-") or default_stem
        stem = f"{truncated}_{digest}"

    return f"{stem[:max_stem_length]}{suffix}"


@asynccontextmanager
async def app_lifespan(server: FastMCP) -> AsyncIterator[None]:
    """Ensures default directories exist when the server starts."""
    global DEFAULT_JSON_DIR, DEFAULT_DOWNLOAD_DIR

    try:
        DEFAULT_JSON_DIR.mkdir(parents=True, exist_ok=True)
        DEFAULT_DOWNLOAD_DIR.mkdir(parents=True, exist_ok=True)
    except OSError:
        # A read-only override must not prevent the desktop's public searches.
        fallback_json = _DEFAULT_OUTPUT_ROOT / "json_files"
        fallback_dl = _DEFAULT_OUTPUT_ROOT / "downloaded_files"
        fallback_json.mkdir(parents=True, exist_ok=True)
        fallback_dl.mkdir(parents=True, exist_ok=True)

        DEFAULT_JSON_DIR = fallback_json
        DEFAULT_DOWNLOAD_DIR = fallback_dl

    yield


# --- Server Initialization ---

mcp = FastMCP(
    "CIPDataverseMCP",
    lifespan=app_lifespan,
)

# --- Resources (read-only examples & guidance) ---
#
# These resources expose canonical JSON shapes for dataset creation/editing so the LLM/client
# can construct valid Dataverse payloads without guessing.


@mcp.resource(
    uri="dataverse://collections/roots",
    name="DataverseRootAliases",
    description="Known top-level Dataverse aliases for CIP Dataverse. Use these with list_dataverse_collections.",
    mime_type="application/json",
    tags={"dataverse", "collections", "help"},
)
def get_known_dataverse_roots() -> dict:
    """Known top-level Dataverse aliases."""
    return {"roots": KNOWN_DATAVERSE_ROOT_ALIASES}


@mcp.resource(
    uri="dataverse://debug/last-error",
    name="DataverseLastHttpError",
    description=(
        "Debug resource exposing the last Dataverse HTTP error captured by MCP tools. "
        "Useful when Dataverse returns opaque 500 errors."
    ),
    mime_type="application/json",
    tags={"dataverse", "debug"},
)
def get_last_dataverse_http_error() -> dict:
    """
    Returns the last captured HTTP error details (if any).
    """
    return {
        "last_error": _LAST_DATAVERSE_HTTP_ERROR,
        "note": (
            "If last_error is null, no tool has captured an HTTP error since server start."
        ),
    }


@mcp.resource(
    uri="dataverse://examples/create/full",
    name="DatasetCreateFullExample",
    description="Full dataset create payload example (expanded fields, metadataBlocks).",
    mime_type="application/json",
    tags={"dataverse", "examples", "dataset", "create"},
)
def get_dataset_create_full_example() -> dict:
    """Returns the full dataset create JSON example shipped with this server."""
    p = DATASET_TEMPLATE_REGISTRY["all_fields_example"]["path"]
    return json.loads(p.read_text(encoding="utf-8"))


@mcp.resource(
    uri="dataverse://examples/edit/metadata",
    name="DatasetEditMetadataExample",
    description="Sample /editMetadata payload (top-level fields array) for partial updates.",
    mime_type="application/json",
    tags={"dataverse", "examples", "dataset", "edit"},
)
def get_dataset_edit_metadata_example() -> dict:
    """Returns a sample editMetadata payload shipped with this server."""
    p = DATASET_TEMPLATE_REGISTRY["edit_metadata_sample"]["path"]
    return json.loads(p.read_text(encoding="utf-8"))


@mcp.resource(
    uri="dataverse://schema/dataset",
    name="DatasetVersionSchemaExample",
    description="JSON schema describing a datasetVersion payload shape (license + citation metadataBlocks).",
    mime_type="application/json",
    tags={"dataverse", "schema", "dataset"},
)
def get_dataset_schema_example() -> dict:
    """Returns the dataset schema example JSON shipped with this server."""
    p = DATASET_TEMPLATE_REGISTRY["dataset_schema_example"]["path"]
    return json.loads(p.read_text(encoding="utf-8"))


# --- Resource Templates (dynamic payload skeleton generators) ---
#
# These templates return valid Dataverse payload skeletons using placeholders so users/LLMs
# can fill in values and send them to create_dataset_in_collection or edit_dataset_metadata.


@mcp.resource(
    uri="dataverse://templates/edit/{field}",
    name="EditMetadataTemplate",
    description=(
        "Generates a /editMetadata payload skeleton for a supported field. "
        "Supported fields: dsDescription, publication, keyword_agrovoc, producer_cip, distributor_cip."
    ),
    mime_type="application/json",
    tags={"dataverse", "templates", "dataset", "edit"},
)
def get_edit_metadata_template(field: str) -> dict:
    """
    Template generator for partial editMetadata payloads.

    Returns a dict shaped as:
      { "fields": [ ... ] }
    """
    key = (field or "").strip().lower()

    if key == "dsdescription":
        return {
            "fields": [
                {
                    "typeName": "dsDescription",
                    "value": [
                        {
                            "dsDescriptionValue": {
                                "typeName": "dsDescriptionValue",
                                "value": "REPLACE_ME_DESCRIPTION_TEXT",
                            }
                        }
                    ],
                }
            ]
        }

    if key == "publication":
        # Useful for cipsoftware where Related Publication Citation is required
        return {
            "fields": [
                {
                    "typeName": "publication",
                    "value": [
                        {
                            "publicationCitation": {
                                "typeName": "publicationCitation",
                                "value": "REPLACE_ME_APA_CITATION_OR_PUBLICATION_TEXT",
                            }
                        }
                    ],
                }
            ]
        }

    if key == "keyword_agrovoc":
        return {
            "fields": [
                {
                    "typeName": "keyword",
                    "value": [
                        {
                            "keywordValue": {
                                "typeName": "keywordValue",
                                "value": "REPLACE_ME_KEYWORD",
                            },
                            "keywordVocabulary": {
                                "typeName": "keywordVocabulary",
                                "value": "AGROVOC",
                            },
                            "keywordVocabularyURI": {
                                "typeName": "keywordVocabularyURI",
                                "value": "REPLACE_ME_AGROVOC_URI",
                            },
                        }
                    ],
                }
            ]
        }

    if key == "producer_cip":
        return {
            "fields": [
                {
                    "typeName": "producer",
                    "value": [
                        {
                            "producerName": {
                                "typeName": "producerName",
                                "value": CIP_ORG_NAME,
                            },
                            "producerAbbreviation": {
                                "typeName": "producerAbbreviation",
                                "value": CIP_ORG_ABBREV,
                            },
                            "producerURL": {
                                "typeName": "producerURL",
                                "value": CIP_ORG_URL,
                            },
                            "producerLogoURL": {
                                "typeName": "producerLogoURL",
                                "value": CIP_ORG_LOGO_URL,
                            },
                        }
                    ],
                }
            ]
        }

    if key == "distributor_cip":
        return {
            "fields": [
                {
                    "typeName": "distributor",
                    "value": [
                        {
                            "distributorName": {
                                "typeName": "distributorName",
                                "value": CIP_ORG_NAME,
                            },
                            "distributorAbbreviation": {
                                "typeName": "distributorAbbreviation",
                                "value": CIP_ORG_ABBREV,
                            },
                            "distributorURL": {
                                "typeName": "distributorURL",
                                "value": CIP_ORG_URL,
                            },
                            "distributorLogoURL": {
                                "typeName": "distributorLogoURL",
                                "value": CIP_ORG_LOGO_URL,
                            },
                        }
                    ],
                }
            ]
        }

    raise ValueError(
        f"Unsupported template field: {field!r}. "
        "Supported fields: dsDescription, publication, keyword_agrovoc, producer_cip, distributor_cip."
    )


@mcp.resource(
    uri="dataverse://templates/create/cipsoftware-minimal",
    name="DatasetCreateCipsoftwareMinimalTemplate",
    description=(
        "Generates a minimal dataset create payload for cipsoftware. "
        "Includes required citation fields plus a required related publication (publicationCitation)."
    ),
    mime_type="application/json",
    tags={"dataverse", "templates", "dataset", "create"},
)
def get_create_cipsoftware_minimal_template() -> dict:
    """
    Minimal create payload skeleton for cipsoftware.

    IMPORTANT:
    - cipsoftware requires a Related Publication Citation.
    - License is included in the template, but you may still need to set it in the Dataverse UI
      depending on server configuration.
    """
    return {
        "datasetVersion": {
            "license": {
                "name": "CC0 1.0",
                "uri": "http://creativecommons.org/publicdomain/zero/1.0",
            },
            "metadataBlocks": {
                "citation": {
                    "fields": [
                        {
                            "typeName": "title",
                            "typeClass": "primitive",
                            "multiple": False,
                            "value": "REPLACE_ME_TITLE",
                        },
                        {
                            "typeName": "author",
                            "typeClass": "compound",
                            "multiple": True,
                            "value": [
                                {
                                    "authorName": {
                                        "typeName": "authorName",
                                        "typeClass": "primitive",
                                        "multiple": False,
                                        "value": "REPLACE_ME_AUTHOR_LASTNAME_FIRSTNAME",
                                    },
                                    "authorAffiliation": {
                                        "typeName": "authorAffiliation",
                                        "typeClass": "primitive",
                                        "multiple": False,
                                        "value": CIP_ORG_NAME,
                                    },
                                }
                            ],
                        },
                        {
                            "typeName": "datasetContact",
                            "typeClass": "compound",
                            "multiple": True,
                            "value": [
                                {
                                    "datasetContactName": {
                                        "typeName": "datasetContactName",
                                        "typeClass": "primitive",
                                        "multiple": False,
                                        "value": "REPLACE_ME_CONTACT_NAME",
                                    },
                                    "datasetContactEmail": {
                                        "typeName": "datasetContactEmail",
                                        "typeClass": "primitive",
                                        "multiple": False,
                                        "value": "REPLACE_ME_CONTACT_EMAIL",
                                    },
                                }
                            ],
                        },
                        {
                            "typeName": "dsDescription",
                            "typeClass": "compound",
                            "multiple": True,
                            "value": [
                                {
                                    "dsDescriptionValue": {
                                        "typeName": "dsDescriptionValue",
                                        "typeClass": "primitive",
                                        "multiple": False,
                                        "value": "REPLACE_ME_DESCRIPTION_TEXT",
                                    }
                                }
                            ],
                        },
                        {
                            "typeName": "publication",
                            "typeClass": "compound",
                            "multiple": True,
                            "value": [
                                {
                                    "publicationCitation": {
                                        "typeName": "publicationCitation",
                                        "typeClass": "primitive",
                                        "multiple": False,
                                        "value": "REPLACE_ME_APA_CITATION_OR_PUBLICATION_TEXT",
                                    }
                                }
                            ],
                        },
                        {
                            "typeName": "subject",
                            "typeClass": "controlledVocabulary",
                            "multiple": True,
                            "value": ["Agricultural Sciences"],
                        },
                    ]
                }
            },
        }
    }


# --- Template Discovery Tools ---


@mcp.tool
async def list_dataset_templates(ctx: Context) -> Dict[str, Dict[str, str]]:
    """
    Lists available dataset payload templates bundled with the server.
    """
    templates: Dict[str, Dict[str, str]] = {}
    for name, entry in DATASET_TEMPLATE_REGISTRY.items():
        path = entry.get("path")
        templates[name] = {
            "description": str(entry.get("description") or ""),
            "filename": path.name if path else "",
        }
    return templates


@mcp.tool
async def get_dataset_template(
    ctx: Context,
    name: Annotated[
        str, Field(description="Template key from list_dataset_templates.")
    ],
    save_as: Annotated[
        Optional[str],
        Field(
            description=(
                "Optional filename to write a copy under DEFAULT_JSON_DIR for reuse."
            )
        ),
    ] = None,
) -> Dict[str, Any]:
    """
    Loads a dataset template payload and optionally saves a copy for later calls.
    """
    entry = DATASET_TEMPLATE_REGISTRY.get(name)
    if not entry:
        message = f"Unknown template '{name}'. Use list_dataset_templates first."
        await ctx.error(message)
        return {"status": "error", "message": message}

    try:
        payload = json.loads(entry["path"].read_text(encoding="utf-8"))
    except Exception as e:
        message = f"Failed to read template '{name}': {e}"
        await ctx.error(message)
        return {"status": "error", "message": message}

    result: Dict[str, Any] = {
        "status": "success",
        "template": name,
        "description": entry.get("description"),
        "payload": payload,
    }

    if save_as:
        safe_name = Path(save_as).name
        out_path = DEFAULT_JSON_DIR / safe_name
        try:
            out_path.parent.mkdir(parents=True, exist_ok=True)
            out_path.write_text(json.dumps(payload, indent=2), encoding="utf-8")
            result["saved_to"] = str(out_path)
        except Exception as e:
            message = f"Failed to save template copy '{safe_name}': {e}"
            await ctx.error(message)
            result["status"] = "error"
            result["message"] = message

    return result


@mcp.tool
def build_cipsoftware_payload(
    title: Annotated[str, Field(description="Dataset title.")],
    author_name: Annotated[
        str,
        Field(
            description="Author full name (Lastname, Firstname recommended).",
        ),
    ],
    contact_name: Annotated[str, Field(description="Dataset contact name.")],
    contact_email: Annotated[str, Field(description="Dataset contact email.")],
    description_text: Annotated[str, Field(description="Dataset description text.")],
    publication_citation: Annotated[
        str, Field(description="Required related publication citation for cipsoftware.")
    ],
    subjects: Annotated[
        Optional[List[str]],
        Field(
            description="Controlled vocabulary subjects. Defaults to Agricultural Sciences."
        ),
    ] = None,
    author_affiliation: Annotated[
        Optional[str], Field(description="Optional author affiliation override.")
    ] = None,
    contact_affiliation: Annotated[
        Optional[str], Field(description="Optional contact affiliation override.")
    ] = None,
    license_name: Annotated[
        str, Field(description="License name (default CC0 1.0).")
    ] = "CC0 1.0",
    license_uri: Annotated[
        str,
        Field(
            description="License URI (default http://creativecommons.org/publicdomain/zero/1.0)."
        ),
    ] = "http://creativecommons.org/publicdomain/zero/1.0",
) -> Dict[str, Any]:
    """
    Generates a ready-to-send cipsoftware dataset payload with required fields populated.
    """
    subjects_value = subjects if subjects else ["Agricultural Sciences"]
    author_aff = author_affiliation or CIP_ORG_NAME
    contact_aff = contact_affiliation or CIP_ORG_NAME

    return {
        "datasetVersion": {
            "license": {"name": license_name, "uri": license_uri},
            "metadataBlocks": {
                "citation": {
                    "fields": [
                        {
                            "typeName": "title",
                            "typeClass": "primitive",
                            "multiple": False,
                            "value": title,
                        },
                        {
                            "typeName": "author",
                            "typeClass": "compound",
                            "multiple": True,
                            "value": [
                                {
                                    "authorName": {
                                        "typeName": "authorName",
                                        "typeClass": "primitive",
                                        "multiple": False,
                                        "value": author_name,
                                    },
                                    "authorAffiliation": {
                                        "typeName": "authorAffiliation",
                                        "typeClass": "primitive",
                                        "multiple": False,
                                        "value": author_aff,
                                    },
                                }
                            ],
                        },
                        {
                            "typeName": "datasetContact",
                            "typeClass": "compound",
                            "multiple": True,
                            "value": [
                                {
                                    "datasetContactName": {
                                        "typeName": "datasetContactName",
                                        "typeClass": "primitive",
                                        "multiple": False,
                                        "value": contact_name,
                                    },
                                    "datasetContactEmail": {
                                        "typeName": "datasetContactEmail",
                                        "typeClass": "primitive",
                                        "multiple": False,
                                        "value": contact_email,
                                    },
                                    "datasetContactAffiliation": {
                                        "typeName": "datasetContactAffiliation",
                                        "typeClass": "primitive",
                                        "multiple": False,
                                        "value": contact_aff,
                                    },
                                }
                            ],
                        },
                        {
                            "typeName": "dsDescription",
                            "typeClass": "compound",
                            "multiple": True,
                            "value": [
                                {
                                    "dsDescriptionValue": {
                                        "typeName": "dsDescriptionValue",
                                        "typeClass": "primitive",
                                        "multiple": False,
                                        "value": description_text,
                                    }
                                }
                            ],
                        },
                        {
                            "typeName": "publication",
                            "typeClass": "compound",
                            "multiple": True,
                            "value": [
                                {
                                    "publicationCitation": {
                                        "typeName": "publicationCitation",
                                        "typeClass": "primitive",
                                        "multiple": False,
                                        "value": publication_citation,
                                    }
                                }
                            ],
                        },
                        {
                            "typeName": "subject",
                            "typeClass": "controlledVocabulary",
                            "multiple": True,
                            "value": subjects_value,
                        },
                    ]
                }
            },
        }
    }


# --- Dataclasses for Structured Tool Output ---


#: How many pages one search will fetch before it stops on its own.
#:
#: A bound rather than a cap on results: without it, an instance that reports no
#: `total_count` and never returns an empty page loops forever. At the default
#: page size this is 50,000 datasets, which is larger than CIP's whole holding.
MAX_SEARCH_PAGES = 500


def _dataset_key(item: Dict[str, Any]) -> str:
    """A stable identity for one search result, for deduplicating across pages.

    `global_id` when the record has one, because that is the dataset's own identity
    and survives a record being re-serialised. Otherwise the whole record, which
    cannot produce a false match — a reader that keyed on a field this server does
    not own would start deduplicating the wrong things the day Dataverse renamed
    one.
    """
    identifier = item.get("global_id")
    if isinstance(identifier, str) and identifier.strip():
        return identifier.strip()
    try:
        return json.dumps(item, sort_keys=True, ensure_ascii=False)
    except (TypeError, ValueError):
        return repr(item)


@dataclass
class SearchInfo:
    """Information about a completed Dataverse search."""

    status: str
    message: str
    output_file: Optional[str] = None
    item_count: int = 0
    #: How many datasets Dataverse said matched the query, across every page.
    #:
    #: **The search read this to decide when to stop and never returned it**, so
    #: "found 4,000, returning 29" was byte-identical to "found 29" for the agent,
    #: for the UI, and for the researcher. A caller that cannot tell a complete
    #: answer from a slice cannot know to narrow the query, and a downstream panel
    #: showing 29 of 4,000 looks exactly like a thorough search of a small corpus.
    #:
    #: 0 means Dataverse did not report one, which is itself worth knowing: the
    #: pagination loop cannot bound itself without it.
    total_count: int = 0
    #: Whether every matching dataset was retrieved.
    #:
    #: False when a cap was hit, a page failed part-way, or Dataverse reported no
    #: total. Separate from `status` on purpose: a partial result is a *success*
    #: that a caller must be able to recognise as partial.
    complete: bool = False


@dataclass
class DownloadInfo:
    """Information about a completed file download operation."""

    status: str
    message: str
    downloaded_files: List[str] = field(default_factory=list)


@dataclass
class ReadResult:
    """Content of a successfully read JSON file."""

    file_path: str
    content: List[Dict[str, Any]]


@dataclass
class DatasetFileItem:
    """A single file entry in a dataset version file listing."""

    file_id: int
    filename: str
    content_type: Optional[str] = None
    size: Optional[int] = None
    restricted: Optional[bool] = None
    persistent_id: Optional[str] = None


@dataclass
class DatasetFilesResult:
    """Result of listing files for a dataset (latest version)."""

    status: str
    message: str
    dataset_persistent_id: str
    files: List[DatasetFileItem] = field(default_factory=list)


@dataclass
class CollectionItem:
    """A single Dataverse collection entry."""

    id: int
    identifier: Optional[str]
    title: str
    type: str


@dataclass
class CollectionListResult:
    """Result of listing child collections."""

    status: str
    message: str
    collections: List[CollectionItem] = field(default_factory=list)


@dataclass
class CollectionInfoResult:
    """Result of retrieving a Dataverse collection."""

    status: str
    message: str
    collection: Optional[Dict[str, Any]] = None


@dataclass
class DatasetActionResult:
    """Result of dataset-level operations."""

    status: str
    message: str
    dataset: Optional[Dict[str, Any]] = None


@dataclass
class FileOperationResult:
    """Result of file-level operations."""

    status: str
    message: str
    file_ids: List[int] = field(default_factory=list)
    data: Optional[Dict[str, Any]] = None


@dataclass
class SchemaResult:
    """Result of retrieving a dataset schema for a collection."""

    status: str
    message: str
    schema: Optional[Dict[str, Any]] = None
    output_file: Optional[str] = None


@dataclass
class TemplateInfo:
    """Information about a dataset metadata template."""

    name: str
    description: str
    path: str


@dataclass
class TemplateListResult:
    """Result of listing dataset metadata templates."""

    status: str
    message: str
    templates: List[TemplateInfo] = field(default_factory=list)


# --- Internal helpers ---


def _normalize_base_url(base_url: Optional[str] = None) -> str:
    """Return a sanitized base URL without trailing slash."""
    return (base_url or BASE_URL or "").rstrip("/")


def _build_auth_headers(api_token: Optional[str] = None) -> Dict[str, str]:
    """Construct headers with optional API token."""
    token = ((api_token or "").strip() or None) if api_token is not None else API_KEY
    headers: Dict[str, str] = {"Accept": "application/json"}
    if token:
        headers["X-Dataverse-key"] = token
    return headers


def _load_metadata_payload(
    metadata: Optional[Dict[str, Any]],
    metadata_file: Optional[str],
    metadata_template: Optional[str],
) -> Dict[str, Any]:
    """Load metadata from inline JSON, a file path, or a registered template."""
    provided_sources = [bool(metadata), bool(metadata_file), bool(metadata_template)]
    if sum(provided_sources) == 0:
        raise ValueError(
            "No metadata provided. Supply inline JSON, a metadata_file path, or a metadata_template."
        )
    if sum(provided_sources) > 1:
        raise ValueError(
            "Provide metadata from only one source (inline JSON, metadata_file, or metadata_template)."
        )
    if metadata_file:
        try:
            content = Path(metadata_file).read_text(encoding="utf-8")
            return json.loads(content)
        except Exception as exc:
            raise ValueError(
                f"Failed to read metadata file '{metadata_file}': {exc}"
            ) from exc
    if metadata_template:
        template = DATASET_TEMPLATE_REGISTRY.get(metadata_template)
        if not template:
            raise ValueError(
                f"Unknown metadata_template '{metadata_template}'. "
                f"Call list_dataset_metadata_templates to see available options."
            )
        try:
            content = template["path"].read_text(encoding="utf-8")
            return json.loads(content)
        except Exception as exc:
            raise ValueError(
                f"Failed to load metadata template '{metadata_template}': {exc}"
            ) from exc
    return metadata or {}


# --- Tools ---


@mcp.tool
async def SearchCIPDataverse(
    ctx: Context,
    query: Annotated[
        str, Field(description='The search query (e.g., "lateblight" or "*").')
    ],
    output_filename: Annotated[
        str,
        Field(
            description='Filename for the saved JSON results (e.g., "search_results.json").'
        ),
    ],
    type: Annotated[str, Field(description="Type of item to search for.")] = "dataset",
    per_page: Annotated[
        int, Field(description="Number of results per page (max 1000). ")
    ] = 50,
    start: Annotated[int, Field(description="Offset to start results from.")] = 0,
    sort: Annotated[
        str, Field(description='Field to sort by ("name" or "date").')
    ] = "date",
    order: Annotated[str, Field(description='Sort order ("asc" or "desc").')] = "desc",
    max_results: Annotated[
        Optional[int], Field(description="Maximum number of items to retrieve.")
    ] = None,
    output_dir: Annotated[
        Optional[str],
        Field(
            description="Directory to save the JSON file. Defaults to a server-managed directory."
        ),
    ] = None,
    content_type: Annotated[
        Optional[str],
        Field(
            description="Optional MIME type to filter files, e.g. 'application/pdf'."
        ),
    ] = None,
) -> SearchInfo:
    """
    Performs a multi-page search on the CIP Dataverse and saves the results to a JSON file.
    """
    base_url = "https://data.cipotato.org/api/search"
    all_items: List[Dict[str, Any]] = []
    seen_keys: set = set()
    current = start
    page_num = 0
    total_count_from_api = 0
    complete = False
    partial_error: Optional[str] = None

    output_folder_path = Path(output_dir) if output_dir else DEFAULT_JSON_DIR
    output_folder_path.mkdir(parents=True, exist_ok=True)
    safe_output_filename = _sanitize_output_filename(
        output_filename,
        default_stem="cip_dataverse_search",
        default_suffix=".json",
    )
    output_filepath = output_folder_path / safe_output_filename

    await ctx.report_progress(
        progress=0, total=max_results or 100, message="Initializing search..."
    )

    async with httpx.AsyncClient(verify=False, timeout=20.0) as client:
        while True:
            page_num += 1
            params = {
                "q": query,
                "type": type,
                "per_page": per_page,
                "start": current,
                "sort": sort,
                "order": order,
            }
            if content_type:
                params["contentType"] = content_type
            progress_total = max_results or (
                total_count_from_api
                if total_count_from_api > 0
                else len(all_items) + per_page
            )
            await ctx.report_progress(
                progress=float(len(all_items)),
                total=float(progress_total),
                message=f"Fetching page {page_num}...",
            )
            try:
                headers = _build_auth_headers()
                resp = await client.get(base_url, params=params, headers=headers)
                resp.raise_for_status()
                data = resp.json().get("data", {})
                items = data.get("items", [])
                if not total_count_from_api:
                    total_count_from_api = data.get("total_count", 0)

                # **Deduplicated, because the offset may be advanced conservatively
                # below.** Over-fetching a page is recoverable; skipping one is not.
                for item in items:
                    key = _dataset_key(item)
                    if key in seen_keys:
                        continue
                    seen_keys.add(key)
                    all_items.append(item)

                if not items:
                    # An empty page is the end of the result set under every paging
                    # scheme Dataverse uses.
                    complete = True
                    break
                if max_results and len(all_items) >= max_results:
                    break
                if total_count_from_api and len(all_items) >= total_count_from_api:
                    complete = True
                    break
                if page_num >= MAX_SEARCH_PAGES:
                    # A bound, so an instance that never reports a total and never
                    # returns an empty page cannot spin forever.
                    break

                # **Advance by what arrived, not by what was asked for.** `per_page`
                # is a request: an instance that caps it returns fewer, and stepping
                # the offset by the requested size skips the difference in silence.
                # Asking for 2000 against a server that caps at 1000 dropped every
                # other thousand records with `status="success"`.
                current += len(items) if len(items) < per_page else per_page
            except (httpx.RequestError, httpx.HTTPStatusError, ValueError) as e:
                # **Keep the pages already collected.** This used to `return` from
                # inside the loop, so a timeout on page nine discarded pages one
                # through eight — the whole search lost to its last request.
                #
                # `HTTPStatusError` is **not** a `RequestError` subclass, so a 4xx or
                # 5xx escaped this handler entirely: no SearchInfo, no file, and the
                # previous search's file still on disk for a reader to treat as
                # current. `ValueError` covers a non-JSON error page.
                partial_error = f"Error during Dataverse API call: {e}"
                await ctx.error(partial_error)
                break
    if max_results and len(all_items) > max_results:
        all_items = all_items[:max_results]
        complete = False

    try:
        async with aiofiles.open(output_filepath, "w", encoding="utf-8") as f:
            await f.write(json.dumps(all_items, ensure_ascii=False, indent=4))

        # **The sentence says what was found, not only what was kept.** A caller
        # reading "saved 29 items" cannot tell a thorough search of a small corpus
        # from a sliver of a large one, and the model composing a shortlist is one
        # such caller.
        success_message = (
            f"Successfully saved {len(all_items)} items to {output_filepath.resolve()}"
        )
        if total_count_from_api:
            success_message += f" (Dataverse reported {total_count_from_api} match(es)"
            success_message += ")" if complete else "; this is a partial result)"
        else:
            success_message += " (Dataverse reported no total; completeness unknown)"
        if partial_error:
            success_message += f" — the search stopped early: {partial_error}"
        await ctx.report_progress(
            progress=len(all_items), total=len(all_items), message=success_message
        )
        return SearchInfo(
            status="success",
            message=success_message,
            output_file=str(output_filepath.resolve()),
            item_count=len(all_items),
            total_count=total_count_from_api,
            complete=complete and partial_error is None,
        )
    except IOError as e:
        error_message = f"Error writing to file {output_filepath}: {e}"
        await ctx.error(error_message)
        return SearchInfo(status="error", message=error_message)


@mcp.tool
async def download_dataset_files_by_doi(
    ctx: Context,
    doi: Annotated[
        str, Field(description="The DOI of the dataset, which must start with 'doi:'.")
    ],
    output_dir: Annotated[
        Optional[str],
        Field(
            description="Directory to save downloaded files. Defaults to a server-managed directory."
        ),
    ] = None,
    extract_zip: Annotated[
        bool,
        Field(description="If true, automatically extracts downloaded ZIP archives."),
    ] = True,
) -> DownloadInfo:
    """
    Downloads all files from the latest version of a dataset specified by its DOI.
    """
    if not doi.startswith("doi:"):
        return DownloadInfo(
            status="error", message="Invalid DOI format. It must start with 'doi:'."
        )

    current_output_dir = Path(output_dir) if output_dir else DEFAULT_DOWNLOAD_DIR
    current_output_dir.mkdir(parents=True, exist_ok=True)

    list_files_url = f"https://data.cipotato.org/api/datasets/:persistentId/versions/:latest/files?persistentId={doi}"

    try:
        async with httpx.AsyncClient(verify=False, timeout=30.0) as client:
            await ctx.report_progress(
                progress=10, total=100, message=f"Fetching file list for DOI: {doi}..."
            )
            headers = _build_auth_headers()
            list_response = await client.get(list_files_url, headers=headers)
            list_response.raise_for_status()
            files_data = list_response.json().get("data", [])

            if not files_data:
                message = f"No files found for DOI: {doi}"
                await ctx.warning(message)
                return DownloadInfo(status="no_files", message=message)

            file_ids = [str(f["dataFile"]["id"]) for f in files_data]
            file_details = [
                {
                    "id": str(f["dataFile"]["id"]),
                    "filename": f["dataFile"].get("filename"),
                }
                for f in files_data
            ]

            await ctx.report_progress(
                progress=25,
                total=100,
                message=f"Found {len(file_ids)} file(s). Downloading...",
            )

            download_url = f"https://data.cipotato.org/api/access/datafiles/{','.join(file_ids)}?format=original"
            data_response = await client.get(
                download_url, headers=headers, timeout=120.0
            )
            data_response.raise_for_status()

            content_disposition = data_response.headers.get("Content-Disposition", "")
            filename_from_header = next(
                (
                    part.split("=")[1].strip('"')
                    for part in content_disposition.split(";")
                    if "filename=" in part
                ),
                None,
            )
            is_zip = (
                "application/zip" in data_response.headers.get("Content-Type", "")
                or len(file_ids) > 1
            )

            if is_zip:
                filename = filename_from_header or "downloaded_data.zip"
            else:
                filename = (
                    file_details[0]["filename"] if file_details else "downloaded_file"
                )

            saved_path = current_output_dir / Path(filename).name
            async with aiofiles.open(saved_path, "wb") as f:
                await f.write(data_response.content)
            await ctx.report_progress(
                progress=70, total=100, message=f"Data saved to {saved_path.resolve()}"
            )

            downloaded_files = []
            if is_zip and extract_zip:
                with zipfile.ZipFile(saved_path, "r") as zf:
                    zf.extractall(current_output_dir)
                for detail in file_details:
                    extracted_path = current_output_dir / Path(detail["filename"]).name
                    if extracted_path.exists():
                        downloaded_files.append(str(extracted_path.resolve()))
                message = f"Files extracted to {current_output_dir.resolve()}."
            else:
                downloaded_files.append(str(saved_path.resolve()))
                message = f"File(s) saved to {saved_path.resolve()}."

        await ctx.report_progress(
            progress=100, total=100, message="Download process complete."
        )
        return DownloadInfo(
            status="success", message=message, downloaded_files=downloaded_files
        )

    except httpx.HTTPStatusError as e:
        error_message = f"HTTP error: {e.response.status_code} - {e.response.text}"
        await ctx.error(error_message)
        return DownloadInfo(status="error", message=error_message)
    except Exception as e:
        error_message = f"An unexpected error occurred: {e}"
        await ctx.error(error_message)
        return DownloadInfo(status="error", message=error_message)


@mcp.tool
async def read_search_results(
    file_path: Annotated[
        str,
        Field(description="The full path to the JSON file containing search results."),
    ],
) -> ReadResult:
    """
    Reads and parses a JSON file of search results previously saved by the SearchCIPDataverse tool.
    This tool enforces security by only allowing access to files within designated server directories.
    """
    allowed_dirs = [DEFAULT_JSON_DIR.resolve(), DEFAULT_DOWNLOAD_DIR.resolve()]

    # Sanitize path to prevent traversal attacks
    safe_fname = Path(file_path).name

    for allowed_dir in allowed_dirs:
        potential_path = (allowed_dir / safe_fname).resolve()
        if potential_path.is_file() and potential_path.is_relative_to(allowed_dir):
            try:
                async with aiofiles.open(potential_path, "r", encoding="utf-8") as f:
                    content_str = await f.read()
                content = json.loads(content_str)
                return ReadResult(file_path=str(potential_path), content=content)
            except Exception as e:
                raise IOError(f"Failed to read or parse JSON file '{safe_fname}': {e}")

    raise FileNotFoundError(
        f"File '{safe_fname}' not found in allowed directories or access denied."
    )


@mcp.tool
async def list_dataset_files(
    ctx: Context,
    persistent_id: Annotated[
        str,
        Field(
            description="Persistent ID (e.g., DOI) of the dataset. Must start with 'doi:' or 'hdl:'."
        ),
    ],
    base_url: Annotated[
        Optional[str],
        Field(
            description="Base URL of the Dataverse server. Defaults to DATAVERSE_BASE_URL env var."
        ),
    ] = None,
    api_token: Annotated[
        Optional[str],
        Field(
            description="API token with permission to view the dataset files (required for restricted datasets)."
        ),
    ] = None,
) -> DatasetFilesResult:
    """
    Lists files from the latest version of a dataset specified by its persistent ID.

    Dataverse API:
    - GET /api/datasets/:persistentId/versions/:latest/files?persistentId=doi:...
    """
    if not (persistent_id.startswith("doi:") or persistent_id.startswith("hdl:")):
        message = "Invalid persistent_id format. It must start with 'doi:' or 'hdl:'."
        await ctx.error(message)
        return DatasetFilesResult(
            status="error",
            message=message,
            dataset_persistent_id=persistent_id,
            files=[],
        )

    resolved_base = _normalize_base_url(base_url)
    url = f"{resolved_base}/api/datasets/:persistentId/versions/:latest/files"
    params = {"persistentId": persistent_id}
    headers = _build_auth_headers(api_token)

    async with httpx.AsyncClient(verify=False, timeout=30.0) as client:
        try:
            await ctx.report_progress(
                progress=0,
                total=1,
                message=f"Listing files for {persistent_id}...",
            )
            resp = await client.get(url, headers=headers, params=params)
            resp.raise_for_status()
            payload = resp.json()
            data_section = payload.get("data", payload)

            items: List[DatasetFileItem] = []
            if isinstance(data_section, list):
                for entry in data_section:
                    if not isinstance(entry, dict):
                        continue
                    raw_data_file = entry.get("dataFile")
                    data_file: Dict[str, Any] = (
                        raw_data_file if isinstance(raw_data_file, dict) else {}
                    )

                    file_id = data_file.get("id")
                    filename = data_file.get("filename")
                    if file_id is None or filename is None:
                        continue
                    try:
                        file_id_int = int(file_id)
                    except Exception:
                        continue

                    items.append(
                        DatasetFileItem(
                            file_id=file_id_int,
                            filename=str(filename),
                            content_type=data_file.get("contentType"),
                            size=data_file.get("filesize"),
                            restricted=bool(entry.get("restricted"))
                            if "restricted" in entry
                            else None,
                            persistent_id=data_file.get("persistentId") or None,
                        )
                    )

            message = f"Retrieved {len(items)} file(s) for {persistent_id}."
            await ctx.report_progress(progress=1, total=1, message=message)
            return DatasetFilesResult(
                status="success",
                message=message,
                dataset_persistent_id=persistent_id,
                files=items,
            )
        except httpx.HTTPStatusError as e:
            error_message = f"HTTP error {e.response.status_code} while listing dataset files: {e.response.text}"
            await ctx.error(error_message)
            return DatasetFilesResult(
                status="error",
                message=error_message,
                dataset_persistent_id=persistent_id,
                files=[],
            )
        except Exception as e:
            error_message = f"Unexpected error while listing dataset files: {e}"
            await ctx.error(error_message)
            return DatasetFilesResult(
                status="error",
                message=error_message,
                dataset_persistent_id=persistent_id,
                files=[],
            )


@mcp.tool
async def list_dataverse_collections(
    ctx: Context,
    parent_alias: Annotated[
        str,
        Field(
            description="Alias or database ID of the parent Dataverse collection. Defaults to 'cipdata'."
        ),
    ] = "cipdata",
    base_url: Annotated[
        Optional[str],
        Field(
            description="Base URL of the Dataverse server. Defaults to DATAVERSE_BASE_URL env var."
        ),
    ] = None,
    api_token: Annotated[
        Optional[str], Field(description="API token with access to the collection.")
    ] = None,
) -> CollectionListResult:
    """
    Lists child Dataverse collections (IDs, aliases, titles) under a parent collection.
    """
    resolved_base = _normalize_base_url(base_url)
    url = f"{resolved_base}/api/dataverses/{parent_alias}/contents"
    headers = _build_auth_headers(api_token)

    async with httpx.AsyncClient(verify=False, timeout=20.0) as client:
        try:
            await ctx.report_progress(
                progress=0,
                total=1,
                message=f"Fetching contents for '{parent_alias}'...",
            )
            resp = await client.get(url, headers=headers)
            resp.raise_for_status()
            raw = resp.json()
            data = raw.get("data", raw)
            collections: List[CollectionItem] = []
            for item in data:
                if item.get("type") == "dataverse":
                    collections.append(
                        CollectionItem(
                            id=item.get("id"),
                            identifier=item.get("identifier") or item.get("alias"),
                            title=item.get("title", ""),
                            type=item.get("type", ""),
                        )
                    )
            message = f"Found {len(collections)} collection(s) under '{parent_alias}'."
            await ctx.report_progress(progress=1, total=1, message=message)
            return CollectionListResult(
                status="success", message=message, collections=collections
            )
        except httpx.HTTPStatusError as e:
            error_message = f"HTTP error {e.response.status_code} while listing collections: {e.response.text}"
            await ctx.error(error_message)
            return CollectionListResult(status="error", message=error_message)
        except Exception as e:
            error_message = f"Unexpected error while listing collections: {e}"
            await ctx.error(error_message)
            return CollectionListResult(status="error", message=error_message)


@mcp.tool
async def create_dataverse_collection(
    ctx: Context,
    parent_alias: Annotated[
        str,
        Field(description="Alias or database ID of the parent Dataverse collection."),
    ],
    collection_data: Annotated[
        Dict[str, Any],
        Field(
            description="JSON payload describing the new Dataverse collection (e.g., name, alias, dataverseContacts, description, dataverseType)."
        ),
    ],
    base_url: Annotated[
        Optional[str],
        Field(
            description="Base URL of the Dataverse server. Defaults to DATAVERSE_BASE_URL env var."
        ),
    ] = None,
    api_token: Annotated[
        Optional[str],
        Field(description="API token with permission to create collections."),
    ] = None,
) -> CollectionInfoResult:
    """
    Creates a new Dataverse collection under a specified parent collection.
    Required fields typically include name, alias, and dataverseContacts.
    """
    resolved_base = _normalize_base_url(base_url)
    url = f"{resolved_base}/api/dataverses/{parent_alias}"
    headers = _build_auth_headers(api_token)
    headers["Content-Type"] = "application/json"

    async with httpx.AsyncClient(verify=False, timeout=30.0) as client:
        try:
            await ctx.report_progress(
                progress=0,
                total=1,
                message=f"Creating collection under '{parent_alias}'...",
            )
            resp = await client.post(url, headers=headers, json=collection_data)
            resp.raise_for_status()
            payload = resp.json()
            collection = payload.get("data", payload)
            message = f"Collection created under '{parent_alias}'."
            await ctx.report_progress(progress=1, total=1, message=message)
            return CollectionInfoResult(
                status="success", message=message, collection=collection
            )
        except httpx.HTTPStatusError as e:
            error_message = f"HTTP error {e.response.status_code} while creating collection: {e.response.text}"
            await ctx.error(error_message)
            return CollectionInfoResult(status="error", message=error_message)
        except Exception as e:
            error_message = f"Unexpected error while creating collection: {e}"
            await ctx.error(error_message)
            return CollectionInfoResult(status="error", message=error_message)


@mcp.tool
async def view_dataverse_collection(
    ctx: Context,
    id_or_alias: Annotated[
        str,
        Field(
            description="Database ID, alias, or ':root' for the Dataverse collection to view."
        ),
    ],
    return_owners: Annotated[
        bool, Field(description="Include owning collections in the response.")
    ] = False,
    base_url: Annotated[
        Optional[str],
        Field(
            description="Base URL of the Dataverse server. Defaults to DATAVERSE_BASE_URL env var."
        ),
    ] = None,
    api_token: Annotated[
        Optional[str],
        Field(description="API token if the collection is unpublished or restricted."),
    ] = None,
) -> CollectionInfoResult:
    """
    Retrieves metadata for a Dataverse collection. Supports published and unpublished collections.
    """
    resolved_base = _normalize_base_url(base_url)
    url = f"{resolved_base}/api/dataverses/{id_or_alias}"
    params = {"returnOwners": "true"} if return_owners else None
    headers = _build_auth_headers(api_token)

    async with httpx.AsyncClient(verify=False, timeout=20.0) as client:
        try:
            await ctx.report_progress(
                progress=0, total=1, message=f"Retrieving collection '{id_or_alias}'..."
            )
            resp = await client.get(url, headers=headers, params=params)
            resp.raise_for_status()
            payload = resp.json()
            collection = payload.get("data", payload)
            message = f"Retrieved collection '{id_or_alias}'."
            await ctx.report_progress(progress=1, total=1, message=message)
            return CollectionInfoResult(
                status="success", message=message, collection=collection
            )
        except httpx.HTTPStatusError as e:
            error_message = f"HTTP error {e.response.status_code} while retrieving collection: {e.response.text}"
            await ctx.error(error_message)
            return CollectionInfoResult(status="error", message=error_message)
        except Exception as e:
            error_message = f"Unexpected error while retrieving collection: {e}"
            await ctx.error(error_message)
            return CollectionInfoResult(status="error", message=error_message)


@mcp.tool
async def list_dataset_metadata_templates(ctx: Context) -> TemplateListResult:
    """
    Lists the dataset metadata templates available on the server.
    """
    templates: List[TemplateInfo] = []
    for name, info in DATASET_TEMPLATE_REGISTRY.items():
        path = info.get("path")
        path_str = str(Path(path).resolve()) if path else ""
        exists = Path(path).exists() if path else False
        description = info.get("description", "")
        if exists:
            templates.append(
                TemplateInfo(name=name, description=description, path=path_str)
            )
    message = f"Found {len(templates)} metadata template(s)."
    return TemplateListResult(status="success", message=message, templates=templates)


@mcp.tool
async def get_dataset_schema_for_collection(
    ctx: Context,
    collection_id: Annotated[
        str,
        Field(
            description="Alias or database ID of the Dataverse collection to retrieve the dataset schema for."
        ),
    ],
    output_filename: Annotated[
        Optional[str],
        Field(
            description="Optional filename to save the schema JSON. Defaults to not writing a file."
        ),
    ] = None,
    output_dir: Annotated[
        Optional[str],
        Field(
            description="Directory to save the schema JSON if output_filename is provided. Defaults to a server-managed directory."
        ),
    ] = None,
    base_url: Annotated[
        Optional[str],
        Field(
            description="Base URL of the Dataverse server. Defaults to DATAVERSE_BASE_URL env var."
        ),
    ] = None,
    api_token: Annotated[
        Optional[str],
        Field(description="API token with permission to view the collection's schema."),
    ] = None,
) -> SchemaResult:
    """
    Retrieves the dataset JSON schema for a Dataverse collection and optionally saves it.
    """
    resolved_base = _normalize_base_url(base_url)
    url = f"{resolved_base}/api/dataverses/{collection_id}/datasetSchema"
    headers = _build_auth_headers(api_token)

    async with httpx.AsyncClient(verify=False, timeout=20.0) as client:
        try:
            await ctx.report_progress(
                progress=0,
                total=1,
                message=f"Fetching dataset schema for '{collection_id}'...",
            )
            resp = await client.get(url, headers=headers)
            resp.raise_for_status()
            payload = resp.json()
            schema = payload.get("data", payload)

            output_path = None
            if output_filename:
                dest_dir = Path(output_dir) if output_dir else DEFAULT_JSON_DIR
                dest_dir.mkdir(parents=True, exist_ok=True)
                safe_output_filename = _sanitize_output_filename(
                    output_filename,
                    default_stem=f"{collection_id}_dataset_schema",
                    default_suffix=".json",
                )
                output_path = dest_dir / safe_output_filename
                output_path.write_text(json.dumps(schema, ensure_ascii=False, indent=2))

            message = f"Retrieved dataset schema for '{collection_id}'."
            await ctx.report_progress(progress=1, total=1, message=message)
            return SchemaResult(
                status="success",
                message=message,
                schema=schema,
                output_file=str(output_path.resolve()) if output_path else None,
            )
        except httpx.HTTPStatusError as e:
            error_message = f"HTTP error {e.response.status_code} while retrieving dataset schema: {e.response.text}"
            await ctx.error(error_message)
            return SchemaResult(status="error", message=error_message)
        except Exception as e:
            error_message = f"Unexpected error while retrieving dataset schema: {e}"
            await ctx.error(error_message)
            return SchemaResult(status="error", message=error_message)


@mcp.tool
async def create_dataset_in_collection(
    ctx: Context,
    parent_alias: Annotated[
        str,
        Field(
            description="Alias or database ID of the parent Dataverse collection (e.g., one of the child collections under 'cipdata')."
        ),
    ],
    dataset_metadata: Annotated[
        Optional[Dict[str, Any]],
        Field(
            description="JSON payload describing the dataset metadata. At minimum requires author info; see provided templates."
        ),
    ] = None,
    metadata_file: Annotated[
        Optional[str],
        Field(
            description="Optional path to a JSON file containing the dataset metadata."
        ),
    ] = None,
    metadata_template: Annotated[
        Optional[str],
        Field(
            description="Name of a registered metadata template (see list_dataset_metadata_templates)."
        ),
    ] = None,
    do_not_validate: Annotated[
        bool,
        Field(
            description="If true, skips metadata validation (requires allow-incomplete setting)."
        ),
    ] = False,
    base_url: Annotated[
        Optional[str],
        Field(
            description="Base URL of the Dataverse server. Defaults to DATAVERSE_BASE_URL env var."
        ),
    ] = None,
    api_token: Annotated[
        Optional[str],
        Field(description="API token with permission to add datasets."),
    ] = None,
) -> DatasetActionResult:
    """
    Creates a dataset under a Dataverse collection. Supports incomplete metadata when allowed by server settings.
    """
    try:
        metadata_payload = _load_metadata_payload(
            dataset_metadata, metadata_file, metadata_template
        )
    except ValueError as exc:
        await ctx.error(str(exc))
        return DatasetActionResult(status="error", message=str(exc))
    resolved_base = _normalize_base_url(base_url)
    url = f"{resolved_base}/api/dataverses/{parent_alias}/datasets"
    if do_not_validate:
        url = f"{url}?doNotValidate=true"
    headers = _build_auth_headers(api_token)
    headers["Content-Type"] = "application/json"

    async with httpx.AsyncClient(verify=False, timeout=30.0) as client:
        try:
            await ctx.report_progress(
                progress=0,
                total=1,
                message=f"Creating dataset in '{parent_alias}'...",
            )
            resp = await client.post(url, headers=headers, json=metadata_payload)
            resp.raise_for_status()
            payload = resp.json()
            dataset = payload.get("data", payload)
            message = f"Dataset created in '{parent_alias}'."
            await ctx.report_progress(progress=1, total=1, message=message)
            return DatasetActionResult(
                status="success", message=message, dataset=dataset
            )
        except httpx.HTTPStatusError as e:
            error_message = f"HTTP error {e.response.status_code} while creating dataset: {e.response.text}"
            _set_last_dataverse_http_error(
                operation="create_dataset_in_collection",
                url=str(e.request.url) if e.request else url,
                status_code=int(e.response.status_code),
                response_text=e.response.text,
            )
            await ctx.error(error_message)
            return DatasetActionResult(status="error", message=error_message)
        except Exception as e:
            error_message = f"Unexpected error while creating dataset: {e}"
            await ctx.error(error_message)
            return DatasetActionResult(status="error", message=error_message)


@mcp.tool
async def update_dataset_metadata(
    ctx: Context,
    persistent_id: Annotated[
        str,
        Field(
            description="Persistent ID (e.g., DOI) of the dataset. Must start with 'doi:' or 'hdl:'."
        ),
    ],
    metadata: Annotated[
        Optional[Dict[str, Any]],
        Field(
            description="Full metadata payload to replace draft (license + metadataBlocks, etc.)."
        ),
    ] = None,
    metadata_file: Annotated[
        Optional[str],
        Field(
            description="Optional path to a JSON file containing the metadata payload."
        ),
    ] = None,
    metadata_template: Annotated[
        Optional[str],
        Field(
            description="Name of a registered metadata template (see list_dataset_metadata_templates)."
        ),
    ] = None,
    base_url: Annotated[
        Optional[str],
        Field(
            description="Base URL of the Dataverse server. Defaults to DATAVERSE_BASE_URL env var."
        ),
    ] = None,
    api_token: Annotated[
        Optional[str],
        Field(description="API token with permission to edit the dataset."),
    ] = None,
) -> DatasetActionResult:
    """
    Replaces the draft metadata for a dataset (creates a draft if none exists).
    """
    try:
        payload = _load_metadata_payload(metadata, metadata_file, metadata_template)
    except ValueError as exc:
        await ctx.error(str(exc))
        return DatasetActionResult(status="error", message=str(exc))
    resolved_base = _normalize_base_url(base_url)
    url = f"{resolved_base}/api/datasets/:persistentId/versions/:draft"
    params = {"persistentId": persistent_id}
    headers = _build_auth_headers(api_token)
    headers["Content-Type"] = "application/json"

    async with httpx.AsyncClient(verify=False, timeout=30.0) as client:
        try:
            await ctx.report_progress(
                progress=0,
                total=1,
                message=f"Updating metadata for {persistent_id}...",
            )
            resp = await client.put(url, headers=headers, params=params, json=payload)
            resp.raise_for_status()
            payload = resp.json()
            dataset = payload.get("data", payload)
            message = f"Metadata updated for {persistent_id}."
            await ctx.report_progress(progress=1, total=1, message=message)
            return DatasetActionResult(
                status="success", message=message, dataset=dataset
            )
        except httpx.HTTPStatusError as e:
            error_message = f"HTTP error {e.response.status_code} while updating metadata: {e.response.text}"
            _set_last_dataverse_http_error(
                operation="update_dataset_metadata",
                url=str(e.request.url) if e.request else url,
                status_code=int(e.response.status_code),
                response_text=e.response.text,
            )
            await ctx.error(error_message)
            return DatasetActionResult(status="error", message=error_message)
        except Exception as e:
            error_message = f"Unexpected error while updating metadata: {e}"
            await ctx.error(error_message)
            return DatasetActionResult(status="error", message=error_message)


@mcp.tool
async def edit_dataset_metadata(
    ctx: Context,
    persistent_id: Annotated[
        str,
        Field(
            description="Persistent ID (e.g., DOI) of the dataset. Must start with 'doi:' or 'hdl:'."
        ),
    ],
    metadata: Annotated[
        Optional[Dict[str, Any]],
        Field(
            description="Partial metadata payload. IMPORTANT: This endpoint expects a top-level object with a `fields` array (as documented by Dataverse). For convenience, this tool also accepts a full datasetVersion/metadataBlocks payload and will extract `metadataBlocks.citation.fields`."
        ),
    ] = None,
    metadata_file: Annotated[
        Optional[str],
        Field(
            description="Optional path to a JSON file containing the metadata payload."
        ),
    ] = None,
    metadata_template: Annotated[
        Optional[str],
        Field(
            description="Name of a registered metadata template (see list_dataset_metadata_templates)."
        ),
    ] = None,
    replace: Annotated[
        bool,
        Field(
            description="If true, replace existing field values; if false, only add where blank or multi-valued."
        ),
    ] = False,
    base_url: Annotated[
        Optional[str],
        Field(
            description="Base URL of the Dataverse server. Defaults to DATAVERSE_BASE_URL env var."
        ),
    ] = None,
    api_token: Annotated[
        Optional[str],
        Field(description="API token with permission to edit the dataset."),
    ] = None,
) -> DatasetActionResult:
    """
    Partially edits dataset metadata using Dataverse's `/editMetadata` endpoint.

    Dataverse expects the payload shape:
      { "fields": [ { "typeName": "...", "value": ... }, ... ] }

    This tool will accept either:
    - the expected `{ "fields": [...] }` payload, OR
    - a full datasetVersion/metadataBlocks payload (common when creating datasets),
      from which it will extract `datasetVersion.metadataBlocks.citation.fields`.
    """
    try:
        raw_payload = _load_metadata_payload(metadata, metadata_file, metadata_template)
    except ValueError as exc:
        await ctx.error(str(exc))
        return DatasetActionResult(status="error", message=str(exc))

    # Normalize payload to the shape required by /editMetadata
    payload: Dict[str, Any]
    if isinstance(raw_payload, dict) and "fields" in raw_payload:
        payload = {"fields": raw_payload.get("fields") or []}
    else:
        # Try to extract citation fields from a full dataset payload
        try:
            citation_fields = (
                raw_payload.get("datasetVersion", {})
                .get("metadataBlocks", {})
                .get("citation", {})
                .get("fields", [])
            )
            payload = {"fields": citation_fields}
        except Exception:
            payload = {"fields": []}

    if not payload.get("fields"):
        error_message = (
            "Invalid payload for editMetadata. Expected a top-level object with `fields`, "
            "or a full datasetVersion.metadataBlocks.citation.fields structure containing fields to edit."
        )
        await ctx.error(error_message)
        return DatasetActionResult(status="error", message=error_message)

    resolved_base = _normalize_base_url(base_url)
    url = f"{resolved_base}/api/datasets/:persistentId/editMetadata"
    params = {"persistentId": persistent_id}
    if replace:
        params["replace"] = "true"
    headers = _build_auth_headers(api_token)
    headers["Content-Type"] = "application/json"

    async with httpx.AsyncClient(verify=False, timeout=30.0) as client:
        try:
            await ctx.report_progress(
                progress=0,
                total=1,
                message=f"Editing metadata for {persistent_id} (replace={replace})...",
            )
            resp = await client.put(url, headers=headers, params=params, json=payload)
            resp.raise_for_status()
            resp_payload = resp.json()
            dataset = resp_payload.get("data", resp_payload)
            message = f"Metadata edited for {persistent_id}."
            await ctx.report_progress(progress=1, total=1, message=message)
            return DatasetActionResult(
                status="success", message=message, dataset=dataset
            )
        except httpx.HTTPStatusError as e:
            error_message = f"HTTP error {e.response.status_code} while editing metadata: {e.response.text}"
            _set_last_dataverse_http_error(
                operation="edit_dataset_metadata",
                url=str(e.request.url) if e.request else url,
                status_code=int(e.response.status_code),
                response_text=e.response.text,
            )
            await ctx.error(error_message)
            return DatasetActionResult(status="error", message=error_message)
        except Exception as e:
            error_message = f"Unexpected error while editing metadata: {e}"
            await ctx.error(error_message)
            return DatasetActionResult(status="error", message=error_message)


@mcp.tool
async def add_file_to_dataset(
    ctx: Context,
    persistent_id: Annotated[
        str,
        Field(
            description="Persistent ID (e.g., DOI) of the dataset to add the file to."
        ),
    ],
    file_path: Annotated[str, Field(description="Path to the file to upload.")],
    description: Annotated[
        Optional[str], Field(description="Optional file description.")
    ] = None,
    directory_label: Annotated[
        Optional[str],
        Field(description="Optional directory label (folder path) within the dataset."),
    ] = None,
    categories: Annotated[
        Optional[List[str]],
        Field(description="Optional list of category labels for the file."),
    ] = None,
    restrict: Annotated[
        Optional[bool], Field(description="Whether to restrict the file.")
    ] = None,
    tab_ingest: Annotated[
        Optional[bool],
        Field(
            description="Whether to allow tabular ingest. Defaults to server behavior if omitted."
        ),
    ] = None,
    base_url: Annotated[
        Optional[str],
        Field(
            description="Base URL of the Dataverse server. Defaults to DATAVERSE_BASE_URL env var."
        ),
    ] = None,
    api_token: Annotated[
        Optional[str], Field(description="API token with permission to add files.")
    ] = None,
) -> FileOperationResult:
    """
    Uploads a file to a dataset using its persistent ID.
    """
    resolved_base = _normalize_base_url(base_url)
    url = f"{resolved_base}/api/datasets/:persistentId/add"
    params = {"persistentId": persistent_id}
    headers = _build_auth_headers(api_token)

    file_name = Path(file_path).name
    try:
        file_bytes = Path(file_path).read_bytes()
    except Exception as e:
        error_message = f"Failed to read file '{file_path}': {e}"
        await ctx.error(error_message)
        return FileOperationResult(status="error", message=error_message)

    json_payload: Dict[str, Any] = {}
    if description is not None:
        json_payload["description"] = description
    if directory_label is not None:
        json_payload["directoryLabel"] = directory_label
    if categories:
        json_payload["categories"] = categories
    if restrict is not None:
        json_payload["restrict"] = restrict
    if tab_ingest is not None:
        json_payload["tabIngest"] = tab_ingest

    files = {"file": (file_name, file_bytes)}
    data = {}
    if json_payload:
        data["jsonData"] = json.dumps(json_payload)

    async with httpx.AsyncClient(verify=False, timeout=120.0) as client:
        try:
            await ctx.report_progress(
                progress=0,
                total=1,
                message=f"Uploading file '{file_name}' to {persistent_id}...",
            )
            resp = await client.post(
                url, params=params, headers=headers, data=data, files=files
            )
            resp.raise_for_status()
            payload = resp.json()
            data_section = payload.get("data", payload)
            file_ids: List[int] = []
            if isinstance(data_section, dict):
                files_info = (
                    data_section.get("files") or data_section.get("dataFiles") or []
                )
                if isinstance(files_info, list):
                    for f in files_info:
                        data_file = f.get("dataFile") if isinstance(f, dict) else None
                        if isinstance(data_file, dict) and "id" in data_file:
                            try:
                                file_ids.append(int(data_file["id"]))
                            except Exception:
                                pass
            message = f"Uploaded file '{file_name}' to dataset {persistent_id}."
            await ctx.report_progress(progress=1, total=1, message=message)
            return FileOperationResult(
                status="success",
                message=message,
                file_ids=file_ids,
                data=data_section,
            )
        except httpx.HTTPStatusError as e:
            error_message = f"HTTP error {e.response.status_code} while uploading file: {e.response.text}"
            await ctx.error(error_message)
            return FileOperationResult(status="error", message=error_message)
        except Exception as e:
            error_message = f"Unexpected error while uploading file: {e}"
            await ctx.error(error_message)
            return FileOperationResult(status="error", message=error_message)


@mcp.tool
async def update_file_categories(
    ctx: Context,
    categories: Annotated[
        List[str],
        Field(
            description="List of category names to set on the file (categories will be created if missing)."
        ),
    ],
    file_id: Annotated[
        Optional[int], Field(description="Database ID of the file to update.")
    ] = None,
    file_persistent_id: Annotated[
        Optional[str],
        Field(description="Persistent ID (DOI/Handle) of the file to update."),
    ] = None,
    base_url: Annotated[
        Optional[str],
        Field(
            description="Base URL of the Dataverse server. Defaults to DATAVERSE_BASE_URL env var."
        ),
    ] = None,
    api_token: Annotated[
        Optional[str],
        Field(description="API token with permission to update file metadata."),
    ] = None,
) -> FileOperationResult:
    """
    Updates the categories for an existing file where the target is specified by file_id or file_persistent_id.

    Dataverse API:
    - POST /api/files/{id}/metadata/categories
    - POST /api/files/:persistentId/metadata/categories?persistentId=doi:...
    """
    if (file_id is None and not file_persistent_id) or (
        file_id is not None and file_persistent_id
    ):
        message = "Provide exactly one of file_id or file_persistent_id."
        await ctx.error(message)
        return FileOperationResult(status="error", message=message)

    if not categories:
        message = "Provide at least one category."
        await ctx.error(message)
        return FileOperationResult(status="error", message=message)

    resolved_base = _normalize_base_url(base_url)
    if file_id is not None:
        url = f"{resolved_base}/api/files/{file_id}/metadata/categories"
        params = None
        target_desc = f"file_id={file_id}"
    else:
        url = f"{resolved_base}/api/files/:persistentId/metadata/categories"
        params = {"persistentId": file_persistent_id}
        target_desc = f"persistent_id={file_persistent_id}"

    headers = _build_auth_headers(api_token)
    headers["Content-Type"] = "application/json"

    body = {"categories": categories}

    async with httpx.AsyncClient(verify=False, timeout=30.0) as client:
        try:
            await ctx.report_progress(
                progress=0,
                total=1,
                message=f"Updating categories for {target_desc}...",
            )
            resp = await client.post(url, headers=headers, params=params, json=body)
            resp.raise_for_status()
            payload = resp.json()
            data_section = payload.get("data", payload)
            message = f"Updated categories for {target_desc}."
            await ctx.report_progress(progress=1, total=1, message=message)
            return FileOperationResult(
                status="success",
                message=message,
                file_ids=[file_id] if file_id is not None else [],
                data=data_section
                if isinstance(data_section, dict)
                else {"data": data_section},
            )
        except httpx.HTTPStatusError as e:
            error_message = f"HTTP error {e.response.status_code} while updating file categories: {e.response.text}"
            await ctx.error(error_message)
            return FileOperationResult(status="error", message=error_message)
        except Exception as e:
            error_message = f"Unexpected error while updating file categories: {e}"
            await ctx.error(error_message)
            return FileOperationResult(status="error", message=error_message)


@mcp.tool
async def set_embargo_on_dataset_files(
    ctx: Context,
    persistent_id: Annotated[
        str, Field(description="Persistent ID (e.g., DOI) of the dataset.")
    ],
    file_ids: Annotated[
        List[int], Field(description="List of file database IDs to embargo.")
    ],
    date_available: Annotated[
        str, Field(description="Embargo end date in ISO format (YYYY-MM-DD).")
    ],
    reason: Annotated[
        Optional[str], Field(description="Optional embargo reason.")
    ] = None,
    base_url: Annotated[
        Optional[str],
        Field(
            description="Base URL of the Dataverse server. Defaults to DATAVERSE_BASE_URL env var."
        ),
    ] = None,
    api_token: Annotated[
        Optional[str],
        Field(description="API token with permission to manage embargo."),
    ] = None,
) -> FileOperationResult:
    """
    Sets an embargo on specified files in a dataset.
    """
    resolved_base = _normalize_base_url(base_url)
    url = f"{resolved_base}/api/datasets/:persistentId/files/actions/:set-embargo"
    params = {"persistentId": persistent_id}
    headers = _build_auth_headers(api_token)
    headers["Content-Type"] = "application/json"
    body = {"dateAvailable": date_available, "fileIds": file_ids}
    if reason:
        body["reason"] = reason

    async with httpx.AsyncClient(verify=False, timeout=20.0) as client:
        try:
            await ctx.report_progress(
                progress=0,
                total=1,
                message=f"Setting embargo on {len(file_ids)} file(s) for {persistent_id}...",
            )
            resp = await client.post(url, headers=headers, params=params, json=body)
            resp.raise_for_status()
            payload = resp.json()
            data_section = payload.get("data", payload)
            message = f"Embargo set for files in {persistent_id}."
            await ctx.report_progress(progress=1, total=1, message=message)
            return FileOperationResult(
                status="success",
                message=message,
                file_ids=file_ids,
                data=data_section,
            )
        except httpx.HTTPStatusError as e:
            error_message = f"HTTP error {e.response.status_code} while setting embargo: {e.response.text}"
            await ctx.error(error_message)
            return FileOperationResult(status="error", message=error_message)
        except Exception as e:
            error_message = f"Unexpected error while setting embargo: {e}"
            await ctx.error(error_message)
            return FileOperationResult(status="error", message=error_message)


@mcp.tool
async def unset_embargo_on_dataset_files(
    ctx: Context,
    persistent_id: Annotated[
        str, Field(description="Persistent ID (e.g., DOI) of the dataset.")
    ],
    file_ids: Annotated[
        List[int],
        Field(description="List of file database IDs to remove embargo from."),
    ],
    base_url: Annotated[
        Optional[str],
        Field(
            description="Base URL of the Dataverse server. Defaults to DATAVERSE_BASE_URL env var."
        ),
    ] = None,
    api_token: Annotated[
        Optional[str],
        Field(description="API token with permission to manage embargo."),
    ] = None,
) -> FileOperationResult:
    """
    Removes embargo from specified files in a dataset.
    """
    resolved_base = _normalize_base_url(base_url)
    url = f"{resolved_base}/api/datasets/:persistentId/files/actions/:unset-embargo"
    params = {"persistentId": persistent_id}
    headers = _build_auth_headers(api_token)
    headers["Content-Type"] = "application/json"
    body = {"fileIds": file_ids}

    async with httpx.AsyncClient(verify=False, timeout=20.0) as client:
        try:
            await ctx.report_progress(
                progress=0,
                total=1,
                message=f"Removing embargo on {len(file_ids)} file(s) for {persistent_id}...",
            )
            resp = await client.post(url, headers=headers, params=params, json=body)
            resp.raise_for_status()
            payload = resp.json()
            data_section = payload.get("data", payload)
            message = f"Embargo removed for files in {persistent_id}."
            await ctx.report_progress(progress=1, total=1, message=message)
            return FileOperationResult(
                status="success",
                message=message,
                file_ids=file_ids,
                data=data_section,
            )
        except httpx.HTTPStatusError as e:
            error_message = f"HTTP error {e.response.status_code} while removing embargo: {e.response.text}"
            await ctx.error(error_message)
            return FileOperationResult(status="error", message=error_message)
        except Exception as e:
            error_message = f"Unexpected error while removing embargo: {e}"
            await ctx.error(error_message)
            return FileOperationResult(status="error", message=error_message)


@mcp.tool
async def publish_dataset(
    ctx: Context,
    persistent_id: Annotated[
        str, Field(description="Persistent ID (e.g., DOI) of the dataset to publish.")
    ],
    version_type: Annotated[
        str,
        Field(
            description='Version bump type: "major", "minor", or "updatecurrent" (superusers).'
        ),
    ] = "major",
    assure_is_indexed: Annotated[
        bool,
        Field(description="If true, fail when dataset is awaiting re-indexing."),
    ] = False,
    base_url: Annotated[
        Optional[str],
        Field(
            description="Base URL of the Dataverse server. Defaults to DATAVERSE_BASE_URL env var."
        ),
    ] = None,
    api_token: Annotated[
        Optional[str], Field(description="API token with permission to publish.")
    ] = None,
) -> DatasetActionResult:
    """
    Publishes a dataset, bumping version per the provided type.
    """
    resolved_base = _normalize_base_url(base_url)
    url = f"{resolved_base}/api/datasets/:persistentId/actions/:publish"
    params: Dict[str, Any] = {"persistentId": persistent_id, "type": version_type}
    if assure_is_indexed:
        params["assureIsIndexed"] = "true"
    headers = _build_auth_headers(api_token)

    async with httpx.AsyncClient(verify=False, timeout=30.0) as client:
        try:
            await ctx.report_progress(
                progress=0,
                total=1,
                message=f"Publishing {persistent_id} as {version_type}...",
            )
            resp = await client.post(url, headers=headers, params=params)
            resp.raise_for_status()
            payload = resp.json()
            dataset = payload.get("data", payload)
            message = f"Publish initiated for {persistent_id}."
            await ctx.report_progress(progress=1, total=1, message=message)
            return DatasetActionResult(
                status="success", message=message, dataset=dataset
            )
        except httpx.HTTPStatusError as e:
            error_message = f"HTTP error {e.response.status_code} while publishing dataset: {e.response.text}"
            _set_last_dataverse_http_error(
                operation="publish_dataset",
                url=str(e.request.url) if e.request else url,
                status_code=int(e.response.status_code),
                response_text=e.response.text,
            )
            await ctx.error(error_message)
            return DatasetActionResult(status="error", message=error_message)
        except Exception as e:
            error_message = f"Unexpected error while publishing dataset: {e}"
            await ctx.error(error_message)
            return DatasetActionResult(status="error", message=error_message)


@mcp.tool
async def delete_dataset_draft(
    ctx: Context,
    dataset_id: Annotated[
        Optional[int],
        Field(
            description="Numeric database ID of the dataset whose draft version should be deleted."
        ),
    ] = None,
    persistent_id: Annotated[
        Optional[str],
        Field(
            description="Persistent ID (doi:/hdl:) of the dataset whose draft version should be deleted."
        ),
    ] = None,
    base_url: Annotated[
        Optional[str],
        Field(
            description="Base URL of the Dataverse server. Defaults to DATAVERSE_BASE_URL env var."
        ),
    ] = None,
    api_token: Annotated[
        Optional[str], Field(description="API token with permission to delete drafts.")
    ] = None,
) -> DatasetActionResult:
    """
    Deletes the draft version of a dataset (only affects drafts, not published versions).
    """
    if (dataset_id is None and not persistent_id) or (
        dataset_id is not None and persistent_id
    ):
        message = "Provide exactly one of dataset_id or persistent_id."
        await ctx.error(message)
        return DatasetActionResult(status="error", message=message)

    resolved_base = _normalize_base_url(base_url)
    if dataset_id is not None:
        url = f"{resolved_base}/api/datasets/{dataset_id}/versions/:draft"
        params = None
        target_desc = f"id {dataset_id}"
    else:
        url = f"{resolved_base}/api/datasets/:persistentId/versions/:draft"
        params = {"persistentId": persistent_id}
        target_desc = f"persistentId {persistent_id}"
    headers = _build_auth_headers(api_token)

    async with httpx.AsyncClient(verify=False, timeout=20.0) as client:
        try:
            await ctx.report_progress(
                progress=0,
                total=1,
                message=f"Deleting draft for dataset ({target_desc})...",
            )
            resp = await client.delete(url, headers=headers, params=params)
            resp.raise_for_status()
            payload = resp.json()
            data = payload.get("data", payload)
            message = f"Draft version deleted for dataset ({target_desc})."
            await ctx.report_progress(progress=1, total=1, message=message)
            return DatasetActionResult(status="success", message=message, dataset=data)
        except httpx.HTTPStatusError as e:
            error_message = f"HTTP error {e.response.status_code} while deleting dataset draft: {e.response.text}"
            await ctx.error(error_message)
            return DatasetActionResult(status="error", message=error_message)
        except Exception as e:
            error_message = f"Unexpected error while deleting dataset draft: {e}"
            await ctx.error(error_message)
            return DatasetActionResult(status="error", message=error_message)


@mcp.tool
async def replace_file_in_dataset(
    ctx: Context,
    new_file_path: Annotated[
        str, Field(description="Path to the new file to upload as replacement.")
    ],
    file_id: Annotated[
        Optional[int], Field(description="Database ID of the file to replace.")
    ] = None,
    file_persistent_id: Annotated[
        Optional[str],
        Field(description="Persistent ID (DOI/Handle) of the file to replace."),
    ] = None,
    metadata: Annotated[
        Optional[Dict[str, Any]],
        Field(
            description="Optional jsonData payload (description, categories, forceReplace, etc.)."
        ),
    ] = None,
    base_url: Annotated[
        Optional[str],
        Field(
            description="Base URL of the Dataverse server. Defaults to DATAVERSE_BASE_URL env var."
        ),
    ] = None,
    api_token: Annotated[
        Optional[str], Field(description="API token with permission to replace files.")
    ] = None,
) -> FileOperationResult:
    """
    Replaces an existing file using either file ID or persistent ID.
    """
    if (file_id is None and not file_persistent_id) or (
        file_id is not None and file_persistent_id
    ):
        message = "Provide exactly one of file_id or file_persistent_id."
        await ctx.error(message)
        return FileOperationResult(status="error", message=message)

    resolved_base = _normalize_base_url(base_url)
    if file_id is not None:
        url = f"{resolved_base}/api/files/{file_id}/replace"
        params = None
    else:
        url = f"{resolved_base}/api/files/:persistentId/replace"
        params = {"persistentId": file_persistent_id}

    headers = _build_auth_headers(api_token)

    file_name = Path(new_file_path).name
    try:
        file_bytes = Path(new_file_path).read_bytes()
    except Exception as e:
        error_message = f"Failed to read file '{new_file_path}': {e}"
        await ctx.error(error_message)
        return FileOperationResult(status="error", message=error_message)

    files = {"file": (file_name, file_bytes)}
    data = {}
    if metadata:
        data["jsonData"] = json.dumps(metadata)

    async with httpx.AsyncClient(verify=False, timeout=120.0) as client:
        try:
            await ctx.report_progress(
                progress=0,
                total=1,
                message=f"Replacing file with '{file_name}'...",
            )
            resp = await client.post(
                url, headers=headers, params=params, data=data, files=files
            )
            resp.raise_for_status()
            payload = resp.json()
            data_section = payload.get("data", payload)
            new_file_ids: List[int] = []
            if isinstance(data_section, dict):
                data_file = data_section.get("dataFile")
                if isinstance(data_file, dict) and "id" in data_file:
                    try:
                        new_file_ids.append(int(data_file["id"]))
                    except Exception:
                        pass
            message = f"File replaced with '{file_name}'."
            await ctx.report_progress(progress=1, total=1, message=message)
            return FileOperationResult(
                status="success",
                message=message,
                file_ids=new_file_ids,
                data=data_section,
            )
        except httpx.HTTPStatusError as e:
            error_message = f"HTTP error {e.response.status_code} while replacing file: {e.response.text}"
            await ctx.error(error_message)
            return FileOperationResult(status="error", message=error_message)
        except Exception as e:
            error_message = f"Unexpected error while replacing file: {e}"
            await ctx.error(error_message)
            return FileOperationResult(status="error", message=error_message)


@mcp.tool
async def delete_file_from_dataset(
    ctx: Context,
    file_id: Annotated[
        Optional[int], Field(description="Database ID of the file to delete.")
    ] = None,
    file_persistent_id: Annotated[
        Optional[str],
        Field(description="Persistent ID (DOI/Handle) of the file to delete."),
    ] = None,
) -> FileOperationResult:
    """
    Deletes a file using either file ID or persistent ID.
    """
    if (file_id is None and not file_persistent_id) or (
        file_id is not None and file_persistent_id
    ):
        message = "Provide exactly one of file_id or file_persistent_id."
        await ctx.error(message)
        return FileOperationResult(status="error", message=message)

    resolved_base = _normalize_base_url(None)
    if file_id is not None:
        url = f"{resolved_base}/api/files/{file_id}"
        params = None
    else:
        url = f"{resolved_base}/api/files/:persistentId"
        params = {"persistentId": file_persistent_id}

    headers = _build_auth_headers()

    async with httpx.AsyncClient(verify=False, timeout=20.0) as client:
        try:
            target = (
                f"file_id={file_id}"
                if file_id is not None
                else f"persistent_id={file_persistent_id}"
            )
            await ctx.report_progress(
                progress=0, total=1, message=f"Deleting {target}..."
            )
            resp = await client.delete(url, headers=headers, params=params)
            resp.raise_for_status()
            payload = resp.json()
            data_section = payload.get("data", payload)
            message = f"Deleted {target}."
            await ctx.report_progress(progress=1, total=1, message=message)
            return FileOperationResult(
                status="success", message=message, data=data_section
            )
        except httpx.HTTPStatusError as e:
            error_message = f"HTTP error {e.response.status_code} while deleting file: {e.response.text}"
            await ctx.error(error_message)
            return FileOperationResult(status="error", message=error_message)
        except Exception as e:
            error_message = f"Unexpected error while deleting file: {e}"
            await ctx.error(error_message)
            return FileOperationResult(status="error", message=error_message)


# --- Resources ---


@mcp.resource(uri="file:///{fname}")
def get_saved_search(fname: str) -> str:
    """
    Retrieves the content of a previously saved JSON or downloaded file.
    This resource enforces security by only allowing access to files within designated server directories.
    """
    allowed_dirs = [DEFAULT_JSON_DIR.resolve(), DEFAULT_DOWNLOAD_DIR.resolve()]

    safe_fname = Path(fname).name

    for allowed_dir in allowed_dirs:
        potential_path = (allowed_dir / safe_fname).resolve()
        if potential_path.is_file() and potential_path.is_relative_to(allowed_dir):
            return potential_path.read_text(encoding="utf-8")

    raise FileNotFoundError(
        f"File '{fname}' not found in allowed directories or access denied."
    )


# --- Prompts ---


@mcp.prompt
def dataverse_template_workflow() -> list[PromptMessage]:
    """Guides the assistant to fetch dataset templates via tools instead of pasting JSON."""
    return [
        PromptMessage(
            "Use the Dataverse template tools to avoid manual JSON. Workflow:\n"
            "1) Call `list_dataset_templates` to see available keys and filenames.\n"
            "2) Call `get_dataset_template` with `name` (and optional `save_as`) to fetch the payload and keep a reusable file path.\n"
            "3) For cipsoftware datasets, prefer `build_cipsoftware_payload` to fill required fields (title, author, contact, description, publicationCitation, subjects).\n"
            "4) Create datasets with `create_dataset_in_collection` using the returned payload.\n"
            "5) For changes, use `edit_dataset_metadata` or `update_dataset_metadata` with the saved payload.",
            role="assistant",
        )
    ]


@mcp.prompt
def dataverse_cipsoftware_intake(
    collection_alias: Annotated[
        str, Field(description="Dataverse collection alias to create the dataset in.")
    ],
) -> list[PromptMessage]:
    """Asks the user for the required fields to build and upload a cipsoftware dataset."""
    return [
        PromptMessage(
            "I can prepare a cipsoftware dataset payload and upload it to Dataverse without pasting JSON.",
            role="assistant",
        ),
        PromptMessage(
            f"To create the dataset in '{collection_alias}', please provide:\n"
            "- Title\n"
            "- Author full name (Lastname, Firstname preferred)\n"
            "- Dataset contact name\n"
            "- Dataset contact email\n"
            "- Short description (1-3 sentences)\n"
            "- Related publication citation (APA or similar)\n"
            "- Subjects (list; defaults to Agricultural Sciences)\n"
            "Optional: author/contact affiliation, license name/URI.\n"
            "I will then call `build_cipsoftware_payload`, optionally save it with `get_dataset_template`, and create the dataset.",
            role="assistant",
        ),
    ]


@mcp.prompt
def get_lateblight_datasets_example() -> tuple:
    """Provides an example prompt for searching and summarizing datasets."""
    example_dir = (
        "C:\\temp\\dataverse_results" if os.name == "nt" else "/tmp/dataverse_results"
    )
    return (
        PromptMessage(
            "I can search for datasets on Dataverse, save the results, and then summarize them for you.",
            role="assistant",
        ),
        PromptMessage(
            f"Please perform the following actions:\n"
            f"1. Call `SearchCIPDataverse` to find datasets related to 'lateblight', fetching a maximum of 10. Save the results to 'lateblight_search.json' in '{example_dir}'.\n"
            f"2. After the search results are saved, use the file path returned by the search tool to summarize the title, description, and authors for each dataset in a table.",
            role="user",
        ),
    )


# --- Server Execution ---

if __name__ == "__main__":
    mcp.run(transport="stdio")
