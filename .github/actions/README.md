# Reusable GitHub Actions

This repository now uses shared actions from
[legra-ai/github-actions](https://github.com/legra-ai/github-actions).

## Migration Notice

All local actions have been migrated to use the shared actions
repository. Workflows now reference:

```yaml
uses: legra-ai/github-actions/.github/actions/action-name@main
```

## Available Shared Actions

See the
[shared actions repository](https://github.com/legra-ai/github-actions)
for complete documentation of all available actions.

### Actions Used in This Repository

- `setup-cocogitto` - Install Cocogitto for version management
- `generate-changelog` - Generate changelog from conventional commits

**Note**: The shared `generate-changelog` action uses
`cargo-version-info` instead of `cocogitto` for changelog generation,
providing better integration with Rust projects.

## Usage

```yaml
- name: Setup Cocogitto
  uses: legra-ai/github-actions/.github/actions/setup-cocogitto@main

- name: Generate changelog
  uses: legra-ai/github-actions/.github/actions/generate-changelog@main
  with:
    release-tag: v0.1.0
```

## Versioning

All shared actions support versioning via inputs and environment
variables. See the
[shared actions documentation](https://github.com/legra-ai/github-actions)
for details.
