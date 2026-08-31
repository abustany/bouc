# AGENTS.md

Bouc: very very simple web-based booking system for a shared family cottage.

## Development environment

The development environment is managed as a Nix Devshell. Unless `IN_NIX_SHELL`
is already set in the environment, you need to prefix all commands you run with
`nix develop -c` (eg. `nix develop -c pnpm typecheck`), or to source the output
of `nix print-dev-env` in the shell to be able to run commands without `nix
develop -c`.

This repository may be checked out using the jj (jujutsu) VCS, maybe as a jj
workspace. Use `jj git root` to find the value of `GIT_DIR` if you need to run
git commands.

## Tech stack

Language: Rust
Data storage: SQLite
Error handling: anyhow for general errors, thiserror when we want to know what failed
Logging: tracing + env_logger
UI: HTML UI served by an axum server, using maud for templating and htmx to add interactivity. Styling is done using tailwind classes on maud elements.

## Coding guidelines

Prefer simple to fancy.

Absolutely NO COMMENTS to tell what the code is doing. The ONLY valid reason to
comment is to add context that can NOT be inferred from the code.

NO unchecked casts: use the From/Into/TryInto trait methods.

When writing e2e tests: use the testing-library module to match elements by
name. If impossible, you may revert to a data-testid attribute.

## Quality checks

tests pass, clippy doesn't complain, code is formatted using `cargo fmt`.
