# Security Policy

## Reporting a Vulnerability

Do **not** report security vulnerabilities in public GitHub Issues or Discussions.

Use [GitHub Private Vulnerability Reporting](https://github.com/MRKups/file-tile/security/advisories/new) on this repository. Include:
- Affected `file-tile` version and macOS version.
- Step-by-step reproduction instructions.
- Concrete impact (for example: unintended file modification, symlink traversal, or arbitrary code execution).

## Security Model

- `file-tile` runs with the signed-in macOS user's filesystem permissions and macOS TCC folder grants.
- In-app navigation scopes (`Free`, `Pinned`, `Branch`, `Range`) and Safety settings are UI guardrails against accidental navigation or deletion, **not** an OS security sandbox against untrusted local code.
- Diagnostic logs (`~/Library/Application Support/file-tile/logs/file-tile.log`) record static lifecycle and error categories only; they never record browsed file paths, filenames, or file contents.
