# Domain Docs

How engineering skills should consume this project's domain documentation before exploring or changing browser realtime media behavior.

## Before exploring, read these

- `CONTEXT.md` at the project root.
- Relevant accepted decisions under `docs/adr/`.

If either is absent, proceed silently. Domain-modeling skills create these documents when terms or decisions are resolved.

## File structure

This project uses a single-context layout: one root `CONTEXT.md` and project-wide decisions under `docs/adr/`.

## Use the glossary's vocabulary

Use terms exactly as defined in `CONTEXT.md` in issue titles, specifications, implementation work, and tests. Avoid synonyms that the glossary explicitly rejects.

If a required concept is missing, reconsider whether new language is necessary or record the gap for domain modeling.

## Flag ADR conflicts

Surface any conflict with an accepted ADR explicitly rather than silently overriding it.
