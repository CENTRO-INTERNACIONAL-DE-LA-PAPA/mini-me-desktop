# Categories of personal data in agricultural research data

Classify every flagged column into one of these before choosing a treatment.

## Direct identifiers

Identify a person on their own. Remove or pseudonymize them always.

- names of farmers, respondents, household heads, enumerators
- phone numbers, email addresses, postal addresses
- national ID numbers (DNI, NIN, Aadhaar, passport), tax or bank numbers
- exact GPS coordinates of a home, a plot, or an interview
- photos, signatures, voice recordings, device identifiers

## Quasi-identifiers

Do not identify anyone alone, but can in combination. Generalize them until
k-anonymity holds.

- location below district level: village, community, plot name
- birth date, exact age, interview date
- sex, ethnicity, language, marital status
- household size, farm size, herd size, when unusually large or small
- occupation or role when rare in the place (the only teacher, the chief)

## Sensitive attributes

Not identifying, but harmful if linked to a person. Keep only when the
analysis needs them, and check l-diversity when they are kept.

- health, disability, nutrition status
- income, debt, land tenure disputes
- religion, political opinion, migration status
- answers about illegal or stigmatized practices

## Not personal data

Plot-level agronomic measurements, trial designs, genotype and accession
codes, and weather data are not personal data. Codes that stand for people
(`enumerator_03`) are pseudonyms: still personal data if a key exists anywhere.

## Free text

Comments and "other, specify" answers can hold any of the above. Treat a
free-text column as a direct identifier until it is dropped or redacted.
