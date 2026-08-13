# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when
working with code in this repository.

## Related projects

This crate is part of a family of Rust projects that share the same
coding standards, tooling, and workflows:

Cargo plugins:

- `cargo-fmt-toml` - Format and normalize Cargo.toml files
- `cargo-nightly` - Nightly toolchain management
- `cargo-plugin-utils` - Shared utilities for cargo plugins
- `cargo-propagate-features` - Propagate features to dependencies
- `cargo-version-info` - Dynamic version computation

Other Rust crates:

- `dotenvage` - Environment variable management
- `rhusky` - Git hooks manager (like Husky for Rust)

All projects use identical configurations for rustfmt, clippy,
markdownlint, cocogitto, and git hooks. When making changes to
tooling or workflow conventions, apply them consistently across
all repositories.

## Project overview

`cargo-version-info` is a Cargo subcommand providing unified version
management across CI/CD, Rust code, and shell scripts. It replaces
scattered version logic in workflows, bash scripts, and build.rs files.

Key commands:

| Command        | Description                                | Status   |
| -------------- | ------------------------------------------ | -------- |
| `next`         | Calculate next patch version from release  | stable   |
| `current`      | Get current version from Cargo.toml        | stable   |
| `latest`       | Get latest published GitHub release        | stable   |
| `dev`          | Generate dev version from git SHA          | stable   |
| `bump`         | Bump version in Cargo.toml and commit      | stable   |
| `changed`      | Check if version changed since last tag    | stable   |
| `release-page` | Generate release page with badges          | stable   |
| `changelog`    | Generate changelog from commits            | unstable |
| `badge`        | Generate quality metric badges             | unstable |
| `pr-log`       | Generate PR log from merged PRs            | unstable |

Note: Commands marked unstable are experimental and work poorly.

Published to crates.io at https://crates.io/crates/cargo-version-info

## Build commands

```bash
# Build
cargo build

# Run directly (during development)
cargo run -- version-info <command> [OPTIONS]

# Run after installation
cargo version-info <command> [OPTIONS]
```

## Testing and linting

```bash
# Run tests (single-threaded required)
cargo test -- --test-threads=1

# Run a single test
cargo test test_name -- --test-threads=1

# Format check (requires nightly)
cargo +nightly fmt --all -- --check

# Format code
cargo +nightly fmt --all

# Clippy (requires nightly)
cargo +nightly clippy --all-targets --all-features -- -D warnings -W missing-docs
```

## Code style

- **Rust Edition**: 2024, MSRV 1.93.0
- **Formatting**: Uses nightly rustfmt with vertical imports grouped
  by std/external/crate (see `rustfmt.toml`)
- **Clippy**: Nightly with strict settings (max 120 lines/function,
  nesting threshold 5)
- **Disallowed variable names**: foo, bar, baz, qux, i, n
- **Documentation**: All public items must have docs (`-W missing-docs`)

## Architecture

### Source structure

- `src/main.rs` - CLI entry point with clap argument parsing
- `src/lib.rs` - Library exports
- `src/version.rs` - Core version parsing, formatting, comparison
- `src/github.rs` - GitHub API integration using octocrab
- `src/commands/` - 18 command implementations, each with its own
  Args struct

### Bump command architecture

The `bump` subcommand (`src/commands/bump/`) is the most complex:

- `mod.rs` - Main orchestration
- `args.rs` - CLI argument definitions
- `version_update.rs` - TOML manipulation with toml_edit
- `readme_update.rs` - README badge version updates
- `commit.rs` - Git commit orchestration
- `diff.rs` - Diff generation and hunk-level filtering
- `index.rs` - Git index operations using gix
- `tree.rs` - Git tree building
- `hooks.rs` - Pre/post bump hook execution
- `tests.rs` - Unit tests

Key features:

- Hunk-level selective staging commits only version-related changes,
  leaving other uncommitted work untouched.
- Hook support via `[package.metadata.version-info]` in Cargo.toml:
  - `pre_bump_hooks` - commands run after Cargo.toml update, before
    commit
  - `post_bump_hooks` - commands run after commit
  - `additional_files` - extra files to include in version commit
- Updates both `[package]` and `[workspace.package]` version fields
  when both exist with explicit versions.

### Key dependencies

- `clap` - CLI argument parsing with derive macros
- `octocrab` - GitHub API client (with rustls)
- `gix` - Pure-Rust git library (gitoxide)
- `toml_edit` - Preserves TOML formatting during edits
- `tokio` - Async runtime for GitHub API calls
- `cargo_plugin_utils` - Shared utilities for cargo plugins
- `dotenvage` - Environment variable loading (supports encrypted .env)

## Environment variables

- `GITHUB_TOKEN` - GitHub personal access token for API access
  (optional, falls back to `gh` CLI)
- `GITHUB_REPOSITORY` - Repository in `owner/repo` format
  (auto-detected from git remote if not set)
- `BUILD_VERSION` - Override version computation (used in CI)
- `CARGO_PKG_VERSION_OVERRIDE` - Legacy version override

## Version management

Use `cargo version-info bump` for version management. This command
updates Cargo.toml and creates a commit, but does NOT create tags
(tags are created by CI after tests pass).

```bash
cargo version-info bump --patch   # 0.0.1 -> 0.0.2
cargo version-info bump --minor   # 0.1.0 -> 0.2.0
cargo version-info bump --major   # 1.0.0 -> 2.0.0
```

The bump command uses pure Rust for all git operations (no git CLI
required), including SSH commit signing via ssh-agent. See README.md
for details.

**Do NOT use `cog bump`** - it creates local tags which conflict
with CI's tag creation workflow.

**Workflow:**

1. Create PR with version bump commit
2. Merge PR to main
3. CI detects version change, creates tag
4. CI generates changelog from conventional commits (via Cocogitto)
5. CI creates GitHub Release with changelog as release body
6. CI publishes to crates.io and uploads platform binaries

### Build version priority

Version is computed dynamically via `build.rs`:

1. `BUILD_VERSION` env var (CI)
2. `CARGO_PKG_VERSION_OVERRIDE` env var (legacy)
3. Cargo.toml version + git SHA
4. Fallback: `0.0.0-dev-<short-sha>`

## Git workflow

- Commits follow Angular Conventional Commits:
  `<type>(<scope>): <subject>`
- Types: feat, fix, docs, refactor, test, style, perf, build, ci,
  chore, revert
- Use lowercase for type, scope, and subject start
- Never bypass git hooks with `--no-verify`
- Never execute `git push` - user must push manually
- Prefer `git rebase` over `git merge` for linear history

### Git hooks (Rhusky)

Git hooks in `.githooks/` are auto-installed via [Rhusky](https://github.com/legra-ai/rhusky)
during `cargo build`. Rhusky sets Git's `core.hooksPath` to point
to `.githooks/`. Installation is skipped in CI environments.

The hooks enforce:

- **pre-commit**: Runs `cargo +nightly fmt --check` and
  `cargo +nightly clippy` on Rust files
- **commit-msg**: Verifies conventional commit format with mandatory
  scope using Cocogitto (`cog verify`)
- **post-commit**: Verifies commit is signed

If hooks aren't active, run `cargo build` to trigger Rhusky
installation.

## Claude Code skills

Skills are defined in `.claude/skills/` and can be invoked with
`/skill-name`:

- `/commit` - Create commits following Angular Conventional Commits
  format with proper scope naming
- `/release-prep` - Prepare a release including version bump,
  testing, and PR creation
- `/version-bump` - Bump version in Cargo.toml using
  `cargo version-info bump` command
- `/testing` - Run tests, linting, and formatting checks

## Markdown formatting

- Maximum line length: 70 characters
- Use `-` for unordered lists (not `*` or `+`)
- Use sentence case for headers (not Title Case)
- Indent nested lists with 2 spaces
- Surround lists and code blocks with blank lines

### Markdown linting

Configuration is in `.markdownlint.json`:

- Line length: 70 characters (MD013)
- Code blocks: unlimited line length

```bash
markdownlint '**/*.md' --ignore node_modules --ignore target
```
