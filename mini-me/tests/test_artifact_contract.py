"""One fixture, generated from the producer's own types, that the client is tested against.

# Why this exists

The roadmap has asked for this since §223, which *was* this bug: `DatasetArtifactPayload` carried
nine fields, the Rust client kept one truncated title, and four distinct datasets rendered as four
identical rows for as long as the feature existed. Nothing failed, because the Python tests checked
what Python wrote and the Rust tests checked what Rust read, and no test ever compared the two.

Building AutoDiscovery produced six more of the same shape in a row (§254, §257–§259, §261–§262): a
correct component whose output nothing downstream consumed. Every one had passing unit tests, and
every one of those tests constructed its subject and asserted on its output — which cannot test a
join.

# What it does

`artifact-contract.json` is generated **from the `TypedDict` annotations**, one distinct value per
field, and committed. Then:

- this module asserts the committed file still matches what the annotations produce, so **adding a
  field to a payload fails here** until the fixture is regenerated;
- `protocol.rs`'s tests read the same file and assert the decoders surface it, so **a field the
  client silently drops fails there**.

Neither half can drift without the other noticing, which is the property §222 established for the
MCP boundary and this is the same discipline one layer in.

# Regenerating

    MINIME_WRITE_CONTRACT=1 pytest mini-me/tests/test_artifact_contract.py

Then read the diff. A field appearing there that the client should show is a client change too.
"""

from __future__ import annotations

import json
import os
import typing
from pathlib import Path

from typing_extensions import is_typeddict

import pytest

from backend import schemas

#: Where the generated fixture lives — inside the Rust crate, because the client is its main reader
#: and a fixture two directory trees from its consumer is a fixture that rots.
FIXTURE = (
    Path(__file__).resolve().parents[2]
    / "crates"
    / "app"
    / "tests"
    / "fixtures"
    / "artifact-contract.json"
)

#: Every artifact payload, keyed by the bundle slice it lands in. The keys are the ones
#: `ArtifactBundle` declares and `decode_values` reads.
BUCKETS: dict[str, type] = {
    "datasets": schemas.DatasetArtifactPayload,
    "sources": schemas.SourceArtifactPayload,
    "reports": schemas.ReportArtifactPayload,
    "files": schemas.FileArtifactPayload,
    "hypotheses": schemas.HypothesisArtifactPayload,
    "libraries": schemas.LibraryArtifactPayload,
    "analyses": schemas.DataAnalysisArtifactPayload,
    "discoveries": schemas.DiscoveryRunArtifactPayload,
}


def _unwrap(annotation: typing.Any) -> typing.Any:
    """Strip `NotRequired[...]` and `X | None` down to the type a value has to satisfy."""
    origin = typing.get_origin(annotation)
    if origin is typing.NotRequired:
        return _unwrap(typing.get_args(annotation)[0])
    if origin in (typing.Union, getattr(__import__("types"), "UnionType", ())):
        options = [arg for arg in typing.get_args(annotation) if arg is not type(None)]
        return _unwrap(options[0]) if options else str
    return annotation


def _value(field: str, annotation: typing.Any, depth: int = 0) -> typing.Any:
    """A distinct, recognisable value for one field.

    Derived from the field's *name*, so a Rust assertion can say which field it is looking at and a
    value arriving in the wrong slot is visible rather than plausible. Deterministic, because the
    committed fixture is diffed.
    """
    inner = _unwrap(annotation)
    origin = typing.get_origin(inner)

    if origin is list:
        (item,) = typing.get_args(inner) or (str,)
        return [_value(f"{field}-{index}", item, depth + 1) for index in range(2)]

    # A nested payload: recurse, so `theories[].supporting_papers[].doi` is covered too.
    #
    # `typing.is_typeddict`, and not a `hasattr`/`isinstance` guess. The first version tested
    # `not isinstance(inner, type(str))` — and `type(str)` is `type`, which every class satisfies,
    # so the branch never ran and every nested payload came out as a plain string. `papers` was a
    # list of two strings, `decode_documents` found no paths, and the consumer half of this test
    # caught its own generator (§264).
    # `typing_extensions.is_typeddict`, because `schemas.py` imports `TypedDict` from
    # `typing_extensions` (for `NotRequired` on 3.10) and the stdlib predicate answers False for
    # those. Asking the wrong one is how the branch below silently never ran.
    if is_typeddict(inner):
        return {
            name: _value(f"{field}-{name}", hint, depth + 1)
            for name, hint in typing.get_type_hints(inner, include_extras=True).items()
        }

    if inner is int:
        # Distinct per field, and never 0 or 1 — those are the values a decoder defaults to, so a
        # dropped field would still look right.
        return 100 + (sum(ord(char) for char in field) % 800)
    if inner is float:
        return round(0.101 + (sum(ord(char) for char in field) % 800) / 1000, 4)
    if inner is bool:
        return True

    # **Shaped where the consumer validates the shape.** A URL field carrying `contract-link` tests
    # nothing about a decoder that only surfaces resolvable links — `protocol::stable_link` requires
    # `http(s)://` and drops anything else, so the fixture has to offer something it can accept, or
    # the field looks unread when it is merely rejected. The field name still rides along, so the
    # value stays unique and says which slot it came from.
    leaf = field.rsplit("-", 1)[-1]
    if leaf in ("link", "url", "doi_url"):
        return f"https://example.invalid/contract-{field}"
    if leaf == "doi":
        return f"10.5555/contract-{field}"
    if leaf in ("path", "index_path", "chart_path", "relative_path"):
        return f"./contract-{field}.txt"
    return f"contract-{field}"


def _fixture() -> dict[str, typing.Any]:
    """The whole bundle, one entry per bucket, every field populated."""
    bundle: dict[str, typing.Any] = {}
    for bucket, payload in BUCKETS.items():
        annotations = typing.get_type_hints(payload, include_extras=True)
        # Prefixed with the bucket, so `datasets.title` and `reports.title` cannot be confused —
        # a value read out of the wrong slice has to be visible, not plausible.
        bundle[bucket] = [
            {name: _value(f"{bucket}-{name}", hint) for name, hint in annotations.items()}
        ]
    return {"artifacts": bundle}


def test_the_committed_fixture_matches_the_producers_own_types():
    """**The half that catches a new field.**

    Add a field to any artifact payload and this fails until the fixture is regenerated — at which
    point the diff shows the client's authors a field they now have to decide about, rather than
    letting it arrive unread for as long as the feature exists (§223).
    """
    generated = _fixture()
    if os.environ.get("MINIME_WRITE_CONTRACT"):
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        FIXTURE.write_text(json.dumps(generated, indent=2, sort_keys=True) + "\n")
        pytest.skip("fixture regenerated; read the diff")

    assert FIXTURE.exists(), (
        f"{FIXTURE} is missing — regenerate with MINIME_WRITE_CONTRACT=1"
    )
    committed = json.loads(FIXTURE.read_text())
    assert committed == generated, (
        "an artifact payload changed shape. Regenerate with "
        "`MINIME_WRITE_CONTRACT=1 pytest mini-me/tests/test_artifact_contract.py`, then decide "
        "whether the client should read the new field."
    )


def test_every_bundle_slice_that_reaches_the_client_is_covered():
    """A payload defined and not listed here is a payload nothing tests the client against."""
    declared = set(typing.get_type_hints(schemas.ArtifactBundle, include_extras=True))
    # Not artifacts: `edges` is provenance wiring, `project` is the spine, `plan` is a transient
    # carrier. Each is decoded by its own path and tested there.
    not_artifacts = {"edges", "project", "plan"}
    assert set(BUCKETS) == declared - not_artifacts, (
        "ArtifactBundle and this test disagree about which slices are artifacts"
    )


def test_every_field_of_every_payload_appears_in_the_fixture():
    """Belt and braces: the generator walks annotations, so this asserts the walk reached them."""
    bundle = _fixture()["artifacts"]
    for bucket, payload in BUCKETS.items():
        annotations = typing.get_type_hints(payload, include_extras=True)
        entry = bundle[bucket][0]
        missing = sorted(set(annotations) - set(entry))
        assert not missing, f"{bucket}: {missing}"
        # And each value names its own bucket, which is what makes the strings globally unique —
        # whether it is a bare marker or a shaped URL, DOI or path.
        for name, value in entry.items():
            if isinstance(value, str):
                assert f"{bucket}-{name}" in value, (bucket, name, value)


def test_the_values_are_distinct_enough_to_catch_a_swap():
    """A field read into the wrong slot must be visible, not plausible.

    So every string carries its own field name and every integer is unique — two datasets rendering
    identically (§223) was only invisible because the values were interchangeable.
    """
    bundle = _fixture()["artifacts"]
    strings: list[str] = []

    def walk(value):
        if isinstance(value, str):
            strings.append(value)
        elif isinstance(value, dict):
            for item in value.values():
                walk(item)
        elif isinstance(value, list):
            for item in value:
                walk(item)

    walk(bundle)
    assert len(strings) == len(set(strings)), "two fields share a value"
    # Every value carries its own field path, whether it is a bare marker or a shaped URL/DOI/path.
    assert all("contract-" in text for text in strings), [
        text for text in strings if "contract-" not in text
    ]
