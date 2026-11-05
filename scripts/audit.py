"""Supply-chain, execution-surface, and invisible prompt-injection audit for file-tile."""

from __future__ import annotations

from pathlib import Path
import re
import subprocess
import sys
import tomllib


ROOT = Path(__file__).resolve().parent.parent
CRATES_IO_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"
SHA256_HEX = re.compile(r"^[0-9a-f]{64}$")

# Zero-width spaces, bidirectional overrides (CVE-2021-42574 Trojan Source),
# and Unicode tag characters (U+E0000..U+E007F) used for invisible prompt injection.
INVISIBLE_UNICODE = re.compile(
    r"[\u200b-\u200f\u202a-\u202e\u2060-\u206f\ufeff\U000e0000-\U000e007f]"
)

ALLOWED_BUILD_SCRIPTS = {
    Path("crates/core/build.rs"),
}

ALLOWED_COMMAND_FILES = {
    Path("crates/core/build.rs"),
    Path("crates/platform-macos/src/folder_usage.rs"),
    Path("crates/platform-macos/src/opening.rs"),
    Path("crates/platform-macos/src/permissions.rs"),
    Path("crates/core/src/persistence/tests/storage.rs"),
}

RUST_FORBIDDEN_PATTERNS = (
    ("network socket API (`std::net`)", re.compile(r"\b(?:std::net|TcpStream|TcpListener|UdpSocket)\b")),
    ("process spawning (`Command`)", re.compile(r"\b(?:std::process::Command|process::Command|Command::new)\b")),
)

SCRIPT_POISON_PATTERNS = (
    ("remote shell pipe execution", re.compile(r"\b(?:curl|wget)\b[^\n|]*\|\s*(?:sh|bash|zsh|python[23]?)\b")),
    ("dynamic code execution (`eval`/`exec`)", re.compile(r"\b(?:eval|exec)\s*\(")),
    ("encoded payload execution (`base64 -d` / `b64decode`)", re.compile(r"\b(?:base64\s+(?:-d|--decode)|b64decode\s*\()")),
)


def tracked_files(root: Path) -> list[Path]:
    result = subprocess.run(
        ["git", "--no-optional-locks", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    )
    return [root / entry for entry in result.stdout.split("\0") if entry]


def check_invisible_unicode(root: Path, files: list[Path]) -> list[str]:
    errors = []
    for path in files:
        if not path.is_file() or path.is_symlink():
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        match = INVISIBLE_UNICODE.search(text)
        if match is not None:
            codepoint = f"U+{ord(match.group(0)):04X}"
            errors.append(
                f"{path.relative_to(root)}: hidden or bidirectional Unicode character ({codepoint})"
            )
    return errors


def check_build_hooks(root: Path, files: list[Path]) -> list[str]:
    errors = []
    for path in files:
        relative = path.relative_to(root)
        if relative.name == "build.rs" and relative not in ALLOWED_BUILD_SCRIPTS:
            errors.append(f"{relative}: unauthorized Cargo build script (`build.rs`)")
        if relative.parts[:1] == (".cargo",):
            errors.append(f"{relative}: unauthorized `.cargo` configuration override")
    return errors


def check_rust_execution_surface(root: Path, files: list[Path]) -> list[str]:
    errors = []
    for path in files:
        relative = path.relative_to(root)
        if relative.suffix != ".rs" or not path.is_file():
            continue
        text = path.read_text(encoding="utf-8")
        for label, pattern in RUST_FORBIDDEN_PATTERNS:
            if label.startswith("process spawning") and relative in ALLOWED_COMMAND_FILES:
                continue
            if pattern.search(text):
                errors.append(f"{relative}: unauthorized {label}")
    return errors


def check_scripts_and_workflows(root: Path, files: list[Path]) -> list[str]:
    errors = []
    self_rel = Path("scripts/audit.py")
    for path in files:
        relative = path.relative_to(root)
        if relative == self_rel or not path.is_file():
            continue
        if relative.suffix not in {".py", ".yml", ".yaml", ".sh"}:
            continue
        text = path.read_text(encoding="utf-8")
        for label, pattern in SCRIPT_POISON_PATTERNS:
            if pattern.search(text):
                errors.append(f"{relative}: suspicious {label}")
    return errors


def check_cargo_supply_chain(root: Path, files: list[Path]) -> list[str]:
    errors = []
    for path in files:
        relative = path.relative_to(root)
        if relative.name == "Cargo.toml" and path.is_file():
            data = tomllib.loads(path.read_text(encoding="utf-8"))
            for section in ("patch", "replace"):
                if section in data:
                    errors.append(f"{relative}: forbidden `[{section}]` dependency override")
            if data.get("lib", {}).get("proc-macro") is True:
                errors.append(f"{relative}: unauthorized `proc-macro = true` target")
            for section in ("dependencies", "dev-dependencies", "build-dependencies"):
                for name, spec in data.get(section, {}).items():
                    if isinstance(spec, dict) and ("git" in spec or "registry" in spec):
                        errors.append(f"{relative}: non-crates.io dependency source on `{name}`")

    lock_path = root / "Cargo.lock"
    if lock_path.is_file():
        lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
        for pkg in lock.get("package", []):
            name = pkg.get("name", "<unknown>")
            source = pkg.get("source")
            if source is None:
                continue
            if source != CRATES_IO_SOURCE:
                errors.append(f"Cargo.lock: package `{name}` uses untrusted source `{source}`")
            checksum = pkg.get("checksum", "")
            if not isinstance(checksum, str) or SHA256_HEX.fullmatch(checksum) is None:
                errors.append(f"Cargo.lock: package `{name}` is missing a valid SHA-256 checksum")
    return errors


def run_audit(root: Path) -> tuple[int, list[str]]:
    files = tracked_files(root)
    errors = []
    errors.extend(check_invisible_unicode(root, files))
    errors.extend(check_build_hooks(root, files))
    errors.extend(check_rust_execution_surface(root, files))
    errors.extend(check_scripts_and_workflows(root, files))
    errors.extend(check_cargo_supply_chain(root, files))
    return len(files), errors


def main() -> int:
    try:
        count, errors = run_audit(ROOT)
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError, subprocess.SubprocessError) as error:
        print(f"Security audit failed to run: {error}", file=sys.stderr)
        return 1
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        print("Fix the reported security audit findings before committing or pushing.", file=sys.stderr)
        return 1
    print(f"Audited {count} tracked files: zero supply-chain, execution-surface, or Unicode findings.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
