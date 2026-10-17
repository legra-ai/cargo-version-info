# Publication

`CI` is the sole publisher. It uses the shared tested-revision pipeline at
`0bead4673cfb56ba38b46fa3661b053572c425f6`. PRs validate without publishing secrets. Main and the daily schedule
prepare signed versions through the repository-scoped release App. Every test,
artifact, tag and registry operation uses the prepared revision.

Do not bump/tag/publish through the displaced dependency-only workflows. Resume
an interrupted release with the CI workflow dispatch. Existing source tags and
published archives are immutable. The GitHub draft becomes public only after
registry identity and extracted published-package tests pass.
