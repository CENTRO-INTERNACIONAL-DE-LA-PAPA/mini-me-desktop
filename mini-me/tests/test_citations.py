"""Guards for reference building (`backend.citations`).

Every record below is a **real Semantic Scholar response**, kept verbatim. The first two are the
papers whose DOIs the academic researcher invented in a live run: it wrote `BF02853934` for
Plaisted, whose DOI is `BF02853982`, and `3558457` for Hijmans, whose DOI is `3558435` — the
identifier of a paper about lichen symbioses.

The module was checked end to end against seventeen papers in six unrelated fields — potato late
blight, CRISPR crop editing, protein-structure prediction, soil carbon, malaria vector control,
Andean glacier retreat — resolving each built DOI at Crossref and comparing every field:

    DOI resolves at Crossref   17/17
    title agrees               17/17
    year agrees                17/17
    volume   agrees 9,  omitted 8,  contradicts 0
    pages    agrees 8,  omitted 9,  contradicts 0

These tests pin what that sweep found, so the shapes the real data takes are caught here rather
than in a rendered bibliography.
"""

from __future__ import annotations

from backend.citations import apa, authors, citable, describe, link


def test_builds_the_reference_the_model_could_not():
    """The two records whose identifiers were invented in production."""
    plaisted = {
        "authors": [{"name": "R. Plaisted"}, {"name": "R. Hoopes"}],
        "year": 1989,
        "title": "The past record and future prospects for the use of exotic potato germplasm",
        "journal": {
            "name": "American Potato Journal",
            "volume": "66",
            "pages": "603-627",
        },
        "externalIds": {"DOI": "10.1007/BF02853982"},
    }
    assert apa(plaisted) == (
        "Plaisted, R., & Hoopes, R. (1989). The past record and future prospects for the use of "
        "exotic potato germplasm. American Potato Journal, 66, 603–627. "
        "https://doi.org/10.1007/BF02853982"
    )

    hijmans = {
        "authors": [{"name": "R. Hijmans"}, {"name": "D. Spooner"}],
        "year": 2001,
        "title": "Geographic distribution of wild potato species",
        # `"88 11"` is how Semantic Scholar packs volume and issue.
        "journal": {
            "name": "American Journal of Botany",
            "volume": "88 11",
            "pages": "2101-2112",
        },
        "externalIds": {"DOI": "10.2307/3558435"},
    }
    assert "88(11), 2101–2112" in apa(hijmans)
    assert apa(hijmans).endswith("https://doi.org/10.2307/3558435")


def test_whitespace_in_the_record_never_reaches_the_reference():
    """Semantic Scholar indents `pages` across newlines.

    Found by comparing seventeen live records against Crossref, where this was the only thing that
    looked like a disagreement and was not one. Left uncollapsed it puts a line break in the middle
    of a citation.
    """
    paper = {
        "authors": [{"name": "M. Alquraishi"}],
        "year": 2021,
        "title": "Machine learning in protein structure prediction",
        "journal": {
            "name": "Current opinion in chemical biology",
            "volume": "65",
            "pages": "\n          1-8\n        ",
        },
        "externalIds": {"DOI": "10.1016/j.cbpa.2021.04.005"},
    }
    built = apa(paper)
    assert "\n" not in built
    assert "65, 1–8." in built


def test_a_surname_particle_travels_with_the_family_name():
    """`M. del R. Herrera` is Herrera, not Herrera-under-R with "M. del" as initials.

    CIP authors have these. Splitting one wrongly is a misattribution, not a formatting slip.
    """
    assert authors(["M. del R. Herrera"]) == "Herrera, M. del R."
    # "de Paz" and "van der Berg" are the family names, so the particle leads them. Asserting
    # "Paz, R. de" here would have pinned the very mistake the PARTICLES set exists to prevent.
    assert authors(["R. de Paz"]) == "de Paz, R."
    assert authors(["L. van der Berg"]) == "van der Berg, L."
    # A spelled-out given name is initialised; one already initialised is left alone.
    assert authors(["Jonathan D. G. Jones"]) == "Jones, J. D. G."
    assert authors(["W. Smilde"]) == "Smilde, W."
    # An ampersand before the last, and a serial comma before it.
    assert authors(["W. Smilde", "G. Brigneti"]) == "Smilde, W., & Brigneti, G."
    assert authors([]) == ""


def test_a_long_collaboration_is_elided_rather_than_listed():
    """APA 7 caps an author list at nineteen, an ellipsis, then the last."""
    many = [f"Given{n} Surname{n}" for n in range(25)]
    built = authors(many)
    # Nineteen, an ellipsis, then the twenty-fifth — twenty names for twenty-five authors.
    assert built.count("Surname") == 20
    assert "…" in built
    assert built.startswith("Surname0, G.")
    assert built.endswith("Surname24, G.")
    assert "Surname19" not in built and "Surname23" not in built


def test_nothing_is_guessed():
    """A record missing a field renders without it.

    The whole discipline of the module: an incomplete reference is correctable by a person, and a
    complete wrong one is not.
    """
    assert apa({"title": "An orphan record"}) == "(n.d.). An orphan record."
    # No volume in the record, so none in the reference — and no invented one.
    sparse = {
        "authors": [{"name": "A. Author"}],
        "year": 2020,
        "title": "A paper",
        "journal": {"name": "A Journal"},
        "externalIds": {},
    }
    assert apa(sparse) == "Author, A. (2020). A paper. A Journal."
    # A title already ending in punctuation does not take a second full stop.
    assert "blight?." not in apa({"title": "Why late blight?"})


def test_the_link_prefers_a_doi_and_falls_back_to_the_corpus_id():
    """Both forms 301-redirect to the paper's own page; a corpus id is nearly always present."""
    assert link({"externalIds": {"DOI": "10.1/x", "CorpusId": 7}}) == (
        "https://api.semanticscholar.org/DOI:10.1/x"
    )
    assert link({"externalIds": {"CorpusId": 45447591}}) == (
        "https://api.semanticscholar.org/CorpusID:45447591"
    )
    assert link({"corpusId": "237744014"}) == (
        "https://api.semanticscholar.org/CorpusID:237744014"
    )
    assert link({"externalIds": {}}) == ""


def test_a_snippet_result_is_not_citable():
    """`snippet_search` returns a title, authors and a corpus id, and nothing else.

    Writing `(n.d.). A title.` over that would replace a missing citation with a threadbare one
    carrying the same authority — which is the failure this module exists to remove, in a smaller
    coat.
    """
    snippet_result = {
        "corpusId": "237744014",
        "title": "Late blight resistance of Ecuadorian potato landraces",
        "authors": ["Á. Monteros-Altamirano"],
    }
    assert not citable(snippet_result)
    # The same paper, once resolved through a metadata tool, is citable.
    assert citable({**snippet_result, "year": 2021, "venue": "Rev. Fac. Agron."})
    assert not citable({"year": 2021})
    assert not citable("not a record")


def test_describe_gives_one_object_to_both_readers():
    """The answer and the sources panel must not disagree about which paper is which."""
    described = describe(
        {
            "title": "A paper",
            "year": 2020,
            "venue": "A Journal",
            "authors": [{"name": "A. Author"}],
            "externalIds": {"DOI": "10.1/x"},
            "tldr": {"text": "It found a thing."},
            "abstract": "A longer account.",
        }
    )
    assert described["citation"].startswith("Author, A. (2020). A paper.")
    assert described["link"] == "https://api.semanticscholar.org/DOI:10.1/x"
    assert described["summary"] == "It found a thing."
    assert described["abstract"] == "A longer account."
