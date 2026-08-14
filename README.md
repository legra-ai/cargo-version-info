# cargo-version-info

[![Crates.io](https://img.shields.io/crates/v/cargo-version-info.svg)](https://crates.io/crates/cargo-version-info)
[![Documentation](https://docs.rs/cargo-version-info/badge.svg)](https://docs.rs/cargo-version-info)
[![CI](https://github.com/legra-ai/cargo-version-info/actions/workflows/ci.yml/badge.svg)](https://github.com/legra-ai/cargo-version-info/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![Downloads](https://img.shields.io/crates/d/cargo-version-info.svg)](https://crates.io/crates/cargo-version-info)

A Cargo subcommand for unified version management across CI/CD, Rust
code, and shell scripts.

## Overview

`cargo-version-info` provides a single source of truth for version
operations, replacing scattered version logic in GitHub Actions
workflows, bash scripts, Makefiles, and Rust build scripts.

## Installation

### Using cargo-binstall (Recommended)

The fastest way to install pre-built binaries:

```bash
cargo install cargo-binstall
cargo binstall cargo-version-info
```

### Using cargo install

Build from source (slower, requires Rust toolchain):

```bash
cargo install cargo-version-info
```

## Commands

### `cargo version-info next`

Calculate the next patch version from the latest GitHub release.

```bash
# Basic usage (auto-detects repo from git remote)
cargo version-info next

# Specify repository explicitly
cargo version-info next --owner my-org --repo my-project

# Output as tag format
cargo version-info next --format tag

# Output as JSON
cargo version-info next --format json
```

**Output formats:**

- `version` (default): Just the version number (e.g., `0.0.6`)
- `tag`: Version with v prefix (e.g., `v0.0.6`)
- `json`: JSON object with `latest`, `next`, and `next_tag` fields

### `cargo version-info current`

Get the current version from `Cargo.toml`.

```bash
# Read from default Cargo.toml
cargo version-info current

# Specify custom manifest path
cargo version-info current --manifest ./crates/my-crate/Cargo.toml

# Output as JSON
cargo version-info current --format json
```

**Output formats:**

- `version` (default): Just the version number
- `json`: JSON object with `version` field

### `cargo version-info latest`

Get the latest published GitHub release version.

```bash
# Auto-detect repository
cargo version-info latest

# Specify repository
cargo version-info latest --owner legra-ai --repo my-project

# Output as tag format
cargo version-info latest --format tag
```

**Output formats:**

- `version` (default): Just the version number
- `tag`: Version with v prefix
- `json`: JSON object with `version` and `tag` fields

### `cargo version-info dev`

Generate a dev version from the current git SHA.

```bash
# Use current directory
cargo version-info dev

# Specify repository path
cargo version-info dev --repo-path ./some/path

# Output as JSON
cargo version-info dev --format json
```

**Output format:** `0.0.0-dev-<short-sha>` (e.g., `0.0.0-dev-a1b2c3d`)

### `cargo version-info tag`

Generate a tag name from a version string.

```bash
# Generate tag from version
cargo version-info tag 0.1.2

# Output as JSON
cargo version-info tag 0.1.2 --format json
```

**Output:** `v0.1.2`

### `cargo version-info bump`

Bump the version in `Cargo.toml` and create a commit.

```bash
# Bump patch version (0.1.0 -> 0.1.1)
cargo version-info bump --patch

# Bump minor version (0.1.0 -> 0.2.0)
cargo version-info bump --minor

# Bump major version (0.1.0 -> 1.0.0)
cargo version-info bump --major

# Set specific version
cargo version-info bump --version 2.0.0

# Update version without committing
cargo version-info bump --patch --no-commit

# Skip Cargo.lock update
cargo version-info bump --patch --no-lock

# Skip README.md version updates
cargo version-info bump --patch --no-readme
```

**Features:**

- Updates `Cargo.toml` version
- Updates `Cargo.lock` (unless `--no-lock`)
- Updates version badges in `README.md` (unless `--no-readme`)
- Creates a conventional commit: `chore(version): bump X.Y.Z -> X.Y.Z`
- Selective staging - only commits version changes, not other work
- Pure Rust implementation - no git CLI required
- SSH commit signing without external tools

**Pure Rust Git Operations:**

All git operations (commits, tree building, index manipulation) are
performed using [gix](https://github.com/Byron/gitoxide) - a pure Rust
git implementation. This means `cargo version-info bump` works in
environments where the git CLI is not installed, such as minimal
Docker containers or restricted CI runners.

**Selective Staging (Hunk-Level):**

The bump command commits only version-related changes, leaving other
uncommitted work untouched. This applies to all version-related files:

| File        | What gets committed                             |
| ----------- | ----------------------------------------------- |
| Cargo.toml  | Only lines containing version changes           |
| Cargo.lock  | All local workspace package entries (not dependency updates) |
| README.md   | Only `crate-name = "version"` lines             |

```bash
# You have uncommitted changes:
# - Cargo.toml: new dependency added
# - Cargo.lock: dependency update from `cargo update`
# - README.md: typo fix in description

cargo version-info bump --patch
# Creates commit with ONLY the version changes
# Your dependency, lock update, and typo fix remain uncommitted
```

This uses hunk-level diff filtering - the same concept as `git add -p`
but automated to select only version-related hunks.

**Hooks and Additional Files:**

The bump command supports hooks for running custom commands and
including additional files in the version commit. Configure hooks in
`Cargo.toml` under `[package.metadata.version-info]`:

```toml
[package.metadata.version-info]
# Commands to run after Cargo.toml is updated but before commit
pre_bump_hooks = [
    "./scripts/sync-npm-version.sh {{version}}"
]

# Additional files to include in the version bump commit
additional_files = [
    "npm/package.json"
]

# Commands to run after the commit is created
post_bump_hooks = [
    "echo 'Version {{version}} committed'"
]
```

| Option            | Description                                     |
| ----------------- | ----------------------------------------------- |
| `pre_bump_hooks`  | Commands run after Cargo.toml update, before commit |
| `additional_files`| Files to stage and commit with version changes  |
| `post_bump_hooks` | Commands run after commit is created            |

The `{{version}}` placeholder is replaced with the new version string.
Use pre_bump_hooks to update other files (like package.json) and
additional_files to include them in the version commit.

**Commit Signing (No GPG/SSH CLI Required):**

SSH commit signing is implemented in pure Rust using the `ssh-key`
crate. When signing is enabled in git config, the tool signs commits
by communicating directly with ssh-agent - no `ssh-keygen` or `gpg`
CLI tools needed.

```bash
# Configure signing (standard git config)
git config commit.gpgsign true
git config gpg.format ssh
git config user.signingkey ~/.ssh/id_ed25519.pub

# bump will create signed commits without calling git or ssh-keygen
cargo version-info bump --patch
```

GPG signing is not yet implemented (requires gpg-agent).

### `cargo version-info compare`

Compare two versions.

```bash
# Compare versions (outputs true if version1 > version2)
cargo version-info compare 0.1.2 0.1.3

# Output as JSON
cargo version-info compare 0.1.2 0.1.3 --format json

# Output as diff format
cargo version-info compare 0.1.2 0.1.3 --format diff
```

**Output formats:**

- `bool` (default): `true` if version1 > version2, `false` otherwise
- `json`: JSON object with comparison result
- `diff`: Human-readable comparison (e.g., `0.1.2 < 0.1.3`)

## Environment Variables

- `GITHUB_TOKEN`: GitHub personal access token for API access
  (optional, falls back to `gh` CLI)
- `GITHUB_REPOSITORY`: Repository in `owner/repo` format
  (auto-detected from git remote if not set)

## Use Cases

### GitHub Actions

Replace bash scripts in GitHub Actions workflows:

```yaml
- name: Calculate next version
  run: |
    NEXT_VERSION=$(cargo version-info next --format version)
    echo "version=$NEXT_VERSION" >> $GITHUB_OUTPUT
```

### Bash Scripts

Replace version extraction logic:

```bash
# Instead of: VERSION=$(grep '^version' Cargo.toml | ...)
VERSION=$(cargo version-info current --format version)

# Instead of: gh release list ... | jq ...
LATEST=$(cargo version-info latest --format version)
```

### Makefiles

Use in Make targets:

```makefile
VERSION := $(shell cargo version-info current --format version)
NEXT_VERSION := $(shell cargo version-info next --format version)
```

### Rust Build Scripts

Can be called from `build.rs`:

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let output = std::process::Command::new("cargo")
    .args(["version-info", "current", "--format", "version"])
    .output()?;
let version = String::from_utf8(output.stdout)?;
# Ok(())
# }
```

## Integration with Existing Workflows

This tool is designed to replace:

1. **GitHub Actions**: See
   [legra-ai/github-actions](https://github.com/legra-ai/github-actions)
   for `calculate-next-version` and `get-version` actions
2. **Bash scripts**: Version extraction in `.bash/*.sh` files
3. **Rust build scripts**: Version resolution in
   `crates/ekg-util-env/build.rs` (can call this tool)

## License

Copyright © 2026 DataRoad Inc, Delaware, USA, trading as Legra.

Licensed under either the [MIT license](LICENSE-MIT) or the
[Apache License, Version 2.0](LICENSE-APACHE), at your option.
