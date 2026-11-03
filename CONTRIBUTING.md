# Contributing to file-tile

Thank you for checking out `file-tile` and taking the time to share feedback.

## Ways to Help Right Now

- **Bug reports and feature ideas:** Please open a GitHub Issue using the issue templates.
- **Questions, workflow ideas, and general feedback:** Start a thread in GitHub Discussions.
- **Security vulnerabilities:** Please report them privately via [SECURITY.md](SECURITY.md).

## Pull Requests

`file-tile` is in its early stages, and core architecture and design are still settling. To keep iteration fast and avoid wasted effort on your side, external pull requests are discouraged for the moment until a clear public roadmap and contributor workflow emerge.

This is temporary, not permanent. If you run into a bug or rough edge, opening an Issue with clear reproduction steps is the best way to help right now, and small fixes may be invited directly on open Issues.

## Building & Verifying Locally

Requires an Apple Silicon Mac (`arm64`) running macOS 15.0 or newer, Rust (via `rustup`), and Python 3.11+:

```sh
task check
```

Or without [Task](https://taskfile.dev):

```sh
python3 scripts/dev.py docs
python3 scripts/dev.py guard
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
python3 -m unittest discover -s scripts/tests -p "test_*.py" -v
cargo test --locked --workspace
```
