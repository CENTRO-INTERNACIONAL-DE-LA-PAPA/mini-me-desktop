# Metadata Extraction Rules

Use this reference to convert Dataverse metadata into a concise user-facing
summary.

## Core fields to extract

- `persistent_id`
  - Dataverse field: `global_id` on the search result — **copy it, never compose it**.
  - If the record instead carries `protocol`, `authority` and `identifier` as
    separate fields, join them as `<protocol>:<authority>/<identifier>`.
  - **If no such field is present on the record, omit the dataset.** A CIP DOI is
    a citation a researcher pastes into a paper: one you reconstructed from a
    title, from a URL, or from what you already knew about CIP's collections
    will look exactly like one you read, and it has already reached a
    researcher's screen that way (docs §289, §298). There is no shape of guess
    that is better than leaving it out.

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
- DOI or persistent ID — copied from `global_id`, per the rule above
- title
- description
- authors
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
