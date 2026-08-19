"""A library artifact is a slice of one growing thing, and merging must treat it as one.

The failure these come from, verbatim: *"after indexing, the first paper dissapeared thats weird"* —
with `.asta/documents/index.yaml` holding both papers and the panel showing one.
"""

from __future__ import annotations

from backend.schemas import _merge_artifacts, _merge_libraries

INDEX = ".asta/documents"


def _turn(*papers, count=None, summary="indexed"):
    return {
        "action": "index",
        "summary": summary,
        "paper_count": count if count is not None else len(papers),
        "index_path": INDEX,
        "papers": [{"title": title, "path": path} for title, path in papers],
        "query_hint": "",
    }


def _bundle(libraries):
    return {"datasets": [], "sources": [], "reports": [], "files": [], "libraries": libraries}


def test_a_second_indexing_does_not_erase_the_first():
    """§233. Two papers on disk, one in the panel."""
    merged = _merge_artifacts(
        _bundle([_turn(("Ploidy-specific symbiotic interactions", "./phytologist.pdf"))]),
        _bundle([_turn(("Graph neural networks", "./Graph-neural-networks.pdf"), count=2)]),
    )
    libraries = merged["libraries"]
    assert len(libraries) == 1, "one index, one artifact"
    titles = [paper["title"] for paper in libraries[0]["papers"]]
    assert titles == ["Ploidy-specific symbiotic interactions", "Graph neural networks"]


def test_the_order_documents_arrived_in_is_the_order_they_keep():
    merged = _merge_libraries(
        [_turn(("First", "a.pdf")), _turn(("Second", "b.pdf")), _turn(("Third", "c.pdf"))]
    )
    assert [p["path"] for p in merged[0]["papers"]] == ["a.pdf", "b.pdf", "c.pdf"]


def test_re_indexing_the_same_paper_is_one_row():
    """Re-indexing is how a library is repaired; it must not double the list."""
    merged = _merge_libraries([_turn(("A paper", "a.pdf")), _turn(("A paper", "a.pdf"))])
    assert len(merged[0]["papers"]) == 1


def test_a_search_turn_does_not_shrink_the_library():
    """`papers` is "the ones just indexed, **or the search matches**" — a search reports fewer."""
    merged = _merge_libraries(
        [
            _turn(("First", "a.pdf"), ("Second", "b.pdf"), count=2),
            _turn(("Second", "b.pdf"), count=2, summary="1 match"),
        ]
    )
    assert len(merged[0]["papers"]) == 2
    # The newest turn still describes what just happened.
    assert merged[0]["summary"] == "1 match"


def test_the_index_may_still_say_it_holds_fewer():
    """`paper_count` is a statement about the index *now*, so removal has to be able to lower it.

    Taking the maximum would be tidier and would lie after `asta documents remove`.
    """
    merged = _merge_libraries(
        [_turn(("A", "a.pdf"), ("B", "b.pdf"), count=2), _turn(count=1, summary="removed one")]
    )
    assert merged[0]["paper_count"] == 1


def test_two_libraries_at_different_roots_stay_apart():
    """`--root` can move the index; two roots are two libraries."""
    other = {**_turn(("Elsewhere", "z.pdf")), "index_path": "other/documents"}
    merged = _merge_libraries([_turn(("Here", "a.pdf")), other])
    assert len(merged) == 2


def test_an_artifact_with_no_index_path_is_dropped_rather_than_merged_into_one():
    """It cannot be attributed to a library, and guessing would put it in the wrong one."""
    assert _merge_libraries([{"papers": [{"title": "Orphan", "path": "o.pdf"}]}]) == []


def test_an_empty_side_leaves_the_other_alone():
    merged = _merge_artifacts(_bundle([_turn(("A", "a.pdf"))]), _bundle([]))
    assert len(merged["libraries"][0]["papers"]) == 1
    merged = _merge_artifacts(_bundle([]), _bundle([_turn(("A", "a.pdf"))]))
    assert len(merged["libraries"][0]["papers"]) == 1
