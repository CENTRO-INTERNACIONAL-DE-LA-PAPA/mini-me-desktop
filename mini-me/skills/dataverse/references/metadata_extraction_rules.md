# Metadata Extraction Rules

Use this reference to convert Dataverse metadata into a concise user-facing
summary.

## Core fields to extract

- `title`
  - Dataverse field: `title`
- `description`
  - Dataverse field: `dsDescription -> dsDescriptionValue`
- `authors`
  - Dataverse field: `author -> authorName`
- `author affiliations`
  - Dataverse field: `author -> authorAffiliation`
- `author identifiers`
  - Dataverse fields: `authorIdentifierScheme`, `authorIdentifier`
- `subjects`
  - Dataverse field: `subject`
- `keywords`
  - Dataverse field: `keyword -> keywordValue`
- `keyword vocabulary`
  - Dataverse fields: `keywordVocabulary`, `keywordVocabularyURI`
- `related publications`
  - Dataverse field: `publication`
  - Useful subfields:
    - `publicationCitation`
    - `publicationIDType`
    - `publicationIDNumber`
    - `publicationURL`
- `producer`
  - Dataverse field: `producer`
- `distributor`
  - Dataverse field: `distributor`
- `time period covered`
  - Dataverse field: `timePeriodCovered`
- `related datasets / material`
  - Dataverse fields: `relatedDatasets`, `relatedMaterial`
- `license`
  - Dataverse path: `datasetVersion -> license`

## Required user-facing summary fields

For recommended datasets, return these whenever available:
- title
- description
- authors
- DOI or persistent ID
- repository or collection context
- subjects or keywords
- file availability
- restricted/public status
- related publications

Use `list_dataset_files` for file availability, filenames, content types, and
restricted/public status when the search results alone are not enough.

Use the search results directly for dataset-level `fileCount` when it is
already present.

## Description rules

- Prefer the main dataset description from `dsDescriptionValue`.
- If multiple descriptions exist, use the most informative one and mention that
  additional description blocks exist when relevant.
- Keep the summary concise. Do not dump the full metadata block unless the user
  asks for it.

## Author rules

- Prefer author names in citation order when available.
- Include affiliations only when they help distinguish similar datasets or add
  credibility/context.
- If author identifiers exist, mention them only when the user needs precise
  attribution.

## Publication rules

- Related publications are important for research reuse.
- Prefer `publicationCitation` as the main readable form.
- Include DOI/identifier type or publication URL when available and useful.

## Missing-field rules

- If a field is not available from the inspected metadata, say so plainly.
- Do not infer missing authors, descriptions, publication links, or licenses.
