"""Standard-library checks and private native fixtures for file-tile."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import shlex
import struct
import subprocess
import sys
import tempfile
import tomllib
import zlib
from urllib.parse import unquote, urlsplit


ROOT = Path(__file__).resolve().parent.parent
EXCLUDED = {
    "target",
    ".git",
    ".agent",
    ".agents",
    ".claude",
    ".gemini",
    ".cursor",
    ".kups",
    "scratch",
    "__pycache__",
    "AGENTS.md",
    "CLAUDE.md",
    "WATCHDOG.md",
    "GEMINI.md",
}
FORBIDDEN_TRACKED_NAMES = {
    ".kups",
    ".agent",
    ".agents",
    ".claude",
    ".gemini",
    ".cursor",
    "scratch",
    "private_keys",
    "AGENTS.md",
    "CLAUDE.md",
    "WATCHDOG.md",
    "GEMINI.md",
    ".DS_Store",
}
FORBIDDEN_KEY_EXTENSIONS = {
    ".p8",
    ".p12",
    ".cer",
    ".key",
    ".pem",
    ".keychain",
    ".keychain-db",
}
FORBIDDEN_ARTIFACT_EXTENSIONS = {
    ".dmg",
    ".raw",
    ".zip",
    ".tar",
    ".gz",
    ".xz",
    ".dylib",
    ".so",
    ".dll",
    ".exe",
    ".rlib",
    ".a",
    ".o",
}
MAX_TRACKED_FILE_BYTES = 2 * 1024 * 1024
CONFLICT_MARKERS = re.compile(r"^(?:<{7} |={7}$|>{7} )", re.MULTILINE)
NOTICE_ENTRY = re.compile(r"^- ([A-Za-z0-9_-]+) ([0-9]+\.[0-9]+\.[0-9]+[^\s|]*) \|", re.MULTILINE)
SECRET_PATTERNS = (
    ("private key material", re.compile(r"-----BEGIN (?:[A-Z0-9]+ )?PRIVATE KEY-----")),
    (
        "GitHub token",
        re.compile(r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{36,}\b|\bgithub_pat_[A-Za-z0-9_]{22,}\b"),
    ),
    ("AWS access key ID", re.compile(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b")),
    (
        "API secret key",
        re.compile(r"\bsk-(?:proj-|ant-)?[A-Za-z0-9_-]{32,}\b|\bAIza[0-9A-Za-z_-]{35}\b"),
    ),
    (
        "host user path",
        re.compile(
            r"/Users/(?!(?:demo|fixture)\b)[A-Za-z0-9._-]+"
            r"|/home/(?!(?:demo|fixture|runner)\b)[A-Za-z0-9._-]+"
            r"|[A-Za-z]:[/\\]Users[/\\](?!(?:demo|fixture)\b)[A-Za-z0-9._-]+"
        ),
    ),
)
ATTRIBUTES = re.compile(r"#\s*!?\s*\[([^\]]*)\]", re.DOTALL)
INLINE_LINKS = re.compile(r"!?\[[^\]\n]*\]\(\s*(<[^>\n]+>|[^\s)]+)(?:\s+[^)\n]*)?\)")
REFERENCE_DEFINITIONS = re.compile(r"^\s{0,3}\[([^\]\n]+)\]:\s*(<[^>\n]+>|\S+)", re.MULTILINE)
REFERENCE_LINKS = re.compile(r"!?\[([^\]\n]+)\]\[([^\]\n]*)\]")
IMPORTS = re.compile(r"^\s*@([^\s\[\]]+\.md)\s*$", re.MULTILINE)
INLINE_CODE = re.compile(r"(?<!`)(`+)(?!`)((?:(?!\n[ \t]*\n).)+?)(?<!`)\1(?!`)", re.DOTALL)
TEST_RESULTS = re.compile(r"^test result: \w+\. (\d+) passed; (\d+) failed;", re.MULTILINE)
PACKAGE_NAME = r"[A-Za-z0-9_-]+"
PACKAGE_VERSION = r"[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?(?:\+[A-Za-z0-9.-]+)?"


def without_fences(source: str) -> str:
    """Exclude fenced examples so they do not become links or headings."""
    lines = []
    fence = None
    for line in source.splitlines():
        marker = re.match(r"^ {0,3}(`{3,}|~{3,})(.*)$", line)
        if fence is not None:
            if marker:
                token = marker.group(1)
                if token[0] == fence[0] and len(token) >= len(fence) and not marker.group(2).strip():
                    fence = None
            lines.append("")
        elif marker:
            token = marker.group(1)
            if token[0] != "`" or "`" not in marker.group(2):
                fence = token
                lines.append("")
            else:
                lines.append(line)
        else:
            lines.append(line)
    return "\n".join(lines)


def heading_anchors(source: str) -> set[str]:
    anchors: set[str] = set()
    for line in without_fences(source).splitlines():
        heading = re.match(r"^\s{0,3}#{1,6}\s+(.+?)\s*#*\s*$", line)
        if not heading:
            continue
        title = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", heading.group(1))
        title = re.sub(r"<[^>]*>", "", title)
        base = re.sub(r"[^\w\- ]", "", title.lower()).replace(" ", "-")
        anchor = base
        suffix = 1
        while anchor in anchors:
            anchor = f"{base}-{suffix}"
            suffix += 1
        anchors.add(anchor)
    return anchors


def reference_key(label: str) -> str:
    return " ".join(label.casefold().split())


def markdown_targets(source: str) -> tuple[list[str], list[str]]:
    source = without_fences(source)
    # Preserve brackets around a code-formatted link label and line boundaries.
    source = INLINE_CODE.sub(lambda match: re.sub(r"[^\n]", " ", match.group(0)), source)
    definitions = {
        reference_key(match.group(1)): match.group(2).strip("<>")
        for match in REFERENCE_DEFINITIONS.finditer(source)
    }
    targets = [match.group(1).strip("<>") for match in INLINE_LINKS.finditer(source)]
    targets.extend(definitions.values())
    targets.extend(match.group(1) for match in IMPORTS.finditer(source))
    errors = []
    for match in REFERENCE_LINKS.finditer(source):
        label = match.group(2) or match.group(1)
        if reference_key(label) not in definitions:
            errors.append(f"undefined Markdown reference: {label}")
    return targets, errors


def exact_local_path(root: Path, target: Path) -> bool:
    """Check actual component spelling even on case-insensitive macOS disks."""
    relative = Path(os.path.abspath(target)).relative_to(root)
    current = root
    for component in relative.parts:
        if not current.is_dir() or component not in {entry.name for entry in current.iterdir()}:
            return False
        current = current / component
    return current.exists()


def raise_scan_error(error: OSError) -> None:
    raise error


def walk_files(root: Path, suffix: str, excluded=()):
    for directory, directories, files in os.walk(root, onerror=raise_scan_error):
        directories[:] = sorted(name for name in directories if name not in excluded)
        for name in sorted(files):
            if name not in excluded and name.endswith(suffix):
                yield Path(directory) / name


def check_documents(root: Path) -> tuple[int, list[str]]:
    root = root.resolve()
    errors = []
    count = 0
    for document in walk_files(root, ".md", EXCLUDED):
        count += 1
        targets, reference_errors = markdown_targets(document.read_text(encoding="utf-8"))
        label = document.relative_to(root)
        errors.extend(f"{label}: {error}" for error in reference_errors)
        for link in targets:
            parsed = urlsplit(link)
            if parsed.scheme or parsed.netloc:
                continue
            target = document.parent / unquote(parsed.path) if parsed.path else document
            try:
                target.resolve().relative_to(root)
                relative = Path(os.path.abspath(target)).relative_to(root)
                if relative.parts[:1] == (".kups",) and not (root / ".kups").exists():
                    continue
                exists = exact_local_path(root, target)
            except ValueError:
                errors.append(f"{label}: local link leaves this checkout: {link}")
                continue
            if not exists:
                errors.append(f"{label}: missing path or filename-case mismatch: {link}")
            elif parsed.fragment and target.suffix == ".md":
                anchors = heading_anchors(target.read_text(encoding="utf-8"))
                if unquote(parsed.fragment) not in anchors:
                    errors.append(f"{label}: missing heading anchor: {link}")
    return count, errors


def source_violations(source: str) -> list[str]:
    violations = []
    for match in ATTRIBUTES.finditer(source):
        attribute = match.group(1)
        if re.search(r"\b(?:allow|expect)\s*\(", attribute) or re.search(
            r"\bignore\s*(?:=|,|\)|$)", attribute
        ):
            violations.append(" ".join(match.group(0).split()))
    return violations


def tracked_repository_files(root: Path) -> list[Path]:
    if (root / ".git").exists():
        result = run_command(["git", "--no-optional-locks", "ls-files", "-z"], cwd=root, merge_stderr=False)
        if result.returncode == 0 and isinstance(result.stdout, str):
            return [
                root / entry
                for entry in result.stdout.split("\0")
                if entry
            ]
    return list(walk_files(root, "", EXCLUDED))


def check_license_notices(root: Path) -> list[str]:
    lock_path = root / "Cargo.lock"
    notices_path = root / "LICENSES" / "THIRD-PARTY-NOTICES.txt"
    if not lock_path.is_file() or not notices_path.is_file():
        return []
    try:
        lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
        notices = notices_path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError):
        return ["LICENSES/THIRD-PARTY-NOTICES.txt: unreadable Cargo.lock or notice file"]
    packages = lock.get("package", [])
    lock_pairs = {(pkg["name"], pkg["version"]) for pkg in packages}
    entries = NOTICE_ENTRY.findall(notices)
    errors = []
    for name, version in entries:
        if name != "Sparkle" and (name, version) not in lock_pairs:
            errors.append(
                f"LICENSES/THIRD-PARTY-NOTICES.txt: stale crate entry {name} {version} not in Cargo.lock (run `task licenses`)"
            )
    workspace_names = {pkg["name"] for pkg in packages if "source" not in pkg}
    notice_names = {name for name, _ in entries}
    for pkg in packages:
        if pkg["name"] in workspace_names:
            for dep in pkg.get("dependencies", []):
                dep_name = dep.split()[0]
                if dep_name not in workspace_names and dep_name not in notice_names:
                    errors.append(
                        f"LICENSES/THIRD-PARTY-NOTICES.txt: missing direct dependency {dep_name} (run `task licenses`)"
                    )
    return errors


def check_workflows(root: Path) -> list[str]:
    workflows_dir = root / ".github" / "workflows"
    if not workflows_dir.is_dir():
        return []
    errors = []
    for path in sorted(workflows_dir.glob("*.yml")):
        relative = path.relative_to(root)
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            errors.append(f"{relative}: unreadable workflow file")
            continue
        if re.search(r"^\s*pull_request_target\s*:", text, re.MULTILINE):
            errors.append(f"{relative}: forbidden pull_request_target trigger")
        if not re.search(r"^permissions:\n  contents: read\b", text, re.MULTILINE):
            errors.append(f"{relative}: missing top-level `permissions: contents: read` declaration")
    return errors


def check_repository_hygiene(root: Path) -> list[str]:
    errors = []
    for path in tracked_repository_files(root):
        relative = path.relative_to(root)
        if any(part in FORBIDDEN_TRACKED_NAMES for part in relative.parts):
            errors.append(f"{relative}: forbidden private or local-only file tracked in repository")
            continue
        suffix = relative.suffix.lower()
        if suffix in FORBIDDEN_KEY_EXTENSIONS:
            errors.append(f"{relative}: forbidden key or certificate file tracked in repository")
            continue
        if suffix in FORBIDDEN_ARTIFACT_EXTENSIONS:
            errors.append(f"{relative}: forbidden binary or archive artifact tracked in repository")
            continue
        if (relative.name == ".env" or relative.name.startswith(".env.")) and not relative.name.endswith(".example"):
            errors.append(f"{relative}: forbidden environment file tracked in repository")
            continue
        if not path.is_file() or path.is_symlink():
            continue
        try:
            size = path.stat().st_size
            if size > MAX_TRACKED_FILE_BYTES:
                errors.append(f"{relative}: tracked file exceeds 2 MiB limit ({size} bytes)")
                continue
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        if CONFLICT_MARKERS.search(text):
            errors.append(f"{relative}: unresolved Git merge-conflict marker")
        for label, pattern in SECRET_PATTERNS:
            if pattern.search(text):
                errors.append(f"{relative}: detected {label}")
    errors.extend(check_license_notices(root))
    errors.extend(check_workflows(root))
    return errors


def check_sources(root: Path) -> tuple[int, list[str]]:
    count = 0
    errors = check_repository_hygiene(root)
    for name in ("src", "crates"):
        directory = root / name
        if not directory.exists():
            continue
        for path in walk_files(directory, ".rs"):
            count += 1
            for attribute in source_violations(path.read_text(encoding="utf-8")):
                errors.append(f"{path.relative_to(root)}: lint suppression or ignored test: {attribute}")
    return count, errors


def executed_test_count(output: str) -> int:
    return sum(int(match.group(1)) + int(match.group(2)) for match in TEST_RESULTS.finditer(output))


def focused_result(returncode: int, output: str) -> int:
    if returncode != 0:
        return returncode if returncode > 0 else 128 - returncode
    return 0 if executed_test_count(output) > 0 else 1


def signal_owned_group(process: subprocess.Popen, signum: int) -> None:
    try:
        os.killpg(process.pid, signum)
    except ProcessLookupError:
        pass


def stop_command(process: subprocess.Popen) -> None:
    signal_owned_group(process, signal.SIGTERM)
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        pass
    finally:
        # The group may still contain children after Cargo itself has exited.
        signal_owned_group(process, signal.SIGKILL)
        try:
            process.wait(timeout=2)
        finally:
            if process.stdout is not None:
                process.stdout.close()
            if process.stderr is not None:
                process.stderr.close()


def interrupt_check(_signum, _frame) -> None:
    raise KeyboardInterrupt


def run_command(command: list[str], *, cwd: Path, merge_stderr: bool = True) -> subprocess.CompletedProcess:
    previous_handlers = {}
    process = None
    try:
        for signum in (signal.SIGINT, signal.SIGTERM):
            previous_handlers[signum] = signal.signal(signum, interrupt_check)
        process = subprocess.Popen(
            command, cwd=cwd, stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT if merge_stderr else subprocess.PIPE,
            text=True, start_new_session=True,
        )
        output, errors = process.communicate()
        return subprocess.CompletedProcess(command, process.returncode, stdout=output, stderr=errors)
    except BaseException:
        if process is not None:
            # A repeated interrupt must not interrupt cleanup of owned work.
            for signum in previous_handlers:
                signal.signal(signum, signal.SIG_IGN)
            stop_command(process)
        raise
    finally:
        for signum, handler in previous_handlers.items():
            signal.signal(signum, handler)


def run_focused(root: Path, selection: str, release: bool) -> int:
    command = ["cargo", "test", "--locked", "--workspace", selection]
    if release:
        command.append("--release")
    result = run_command(command, cwd=root)
    print(result.stdout, end="", flush=True)
    status = focused_result(result.returncode, result.stdout)
    if result.returncode == 0 and status != 0:
        print(
            f"Focused filter {selection!r} executed no tests. Choose an existing Rust test name.",
            file=sys.stderr,
        )
    return status


def nonempty(value: str) -> str:
    if not value.strip():
        raise argparse.ArgumentTypeError("choose a nonempty Rust test filter")
    return value


def report_check(count: int, errors: list[str], subject: str) -> int:
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        print(f"Fix the reported {subject} issues and rerun the check.", file=sys.stderr)
        return 1
    print(f"Checked {count} {subject} files.")
    return 0


def qualification_command(root: Path, command: list[str], label: str) -> str:
    """Reuse owned process-group cleanup, but never publish subprocess diagnostics."""
    try:
        result = run_command(command, cwd=root, merge_stderr=False)
    except (OSError, UnicodeError, subprocess.SubprocessError):
        raise ValueError(f"{label} could not run; check the local toolchain or checkout") from None
    if result.returncode != 0:
        raise ValueError(f"{label} failed (exit {result.returncode}); inspect the command locally")
    if not isinstance(result.stdout, str):
        raise ValueError(f"{label} returned no text")
    return result.stdout


def qualification_file(root: Path, name: str) -> bytes:
    path = root / name
    try:
        if path.is_symlink() or not path.resolve().is_relative_to(root.resolve()):
            raise ValueError("evidence path is outside the checkout or is a symlink")
    except RuntimeError:
        raise ValueError("evidence path cannot be resolved safely") from None
    if path.exists() and not path.is_file():
        raise ValueError("evidence is not a regular file")
    with path.open("rb") as source:
        data = source.read(1024 * 1024 + 1)
    if len(data) > 1024 * 1024:
        raise ValueError("evidence exceeds the 1 MiB report input limit")
    return data


def qualification_token(value, pattern: str) -> str:
    if not isinstance(value, str) or re.fullmatch(pattern, value) is None:
        raise ValueError("evidence has a missing or malformed identity field")
    return value


def target_triple(value: str) -> str:
    if re.fullmatch(r"[A-Za-z0-9_]+(?:-[A-Za-z0-9_]+){2,}", value) is None:
        raise argparse.ArgumentTypeError("choose a target triple, not a target file path")
    return value


def qualification_dependencies(metadata: dict) -> tuple[dict, list[dict]]:
    """Project the reachable graph, not path-bearing Cargo package records."""
    try:
        packages = {package["id"]: package for package in metadata["packages"]}
        nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
        if len(packages) != len(metadata["packages"]) or len(nodes) != len(metadata["resolve"]["nodes"]):
            raise ValueError("duplicate graph identity")
        root = metadata["resolve"]["root"]
        pending = [root]
        reachable = set()
        while pending:
            identity = pending.pop()
            if identity in reachable:
                continue
            reachable.add(identity)
            pending.extend(dependency["pkg"] for dependency in nodes[identity]["deps"])
        records = {}
        for identity in reachable:
            package = packages[identity]
            license = package["license"]
            if license is not None:
                qualification_token(license, r"[A-Za-z0-9().+ /-]+")
                if re.search(r"(?:^|[\s(])/", license):
                    raise ValueError("path-like license declaration")
            features = nodes[identity]["features"]
            if not isinstance(features, list):
                raise ValueError("malformed features")
            records[identity] = {
                "name": qualification_token(package["name"], PACKAGE_NAME),
                "version": qualification_token(package["version"], PACKAGE_VERSION),
                "license_declaration": license,
                "license_file_declared": package.get("license_file") is not None,
                "features": sorted({qualification_token(feature, r"[A-Za-z0-9_+./-]+") for feature in features}),
            }
            if any(feature.startswith("/") or "../" in feature for feature in records[identity]["features"]):
                raise ValueError("path-like feature")
        dependencies = sorted(
            (record for identity, record in records.items() if identity != root),
            key=lambda record: (record["name"], record["version"], json.dumps(record, sort_keys=True)),
        )
        return records[root], dependencies
    except (KeyError, TypeError, ValueError, AttributeError):
        raise ValueError("Cargo metadata graph is missing or malformed; rerun metadata locally") from None


def qualification_future(root: Path) -> dict:
    try:
        data = json.loads(qualification_file(root, "target/.future-incompat-report.json"))
        if type(data["version"]) is not int or data["version"] != 0 or not isinstance(data["reports"], list):
            raise ValueError("unsupported report shape")
        reports = []
        for report in data["reports"]:
            if type(report["id"]) is not int or not isinstance(report["per_package"], dict):
                raise ValueError("malformed report")
            packages = []
            for identity, diagnostic in report["per_package"].items():
                match = re.fullmatch(f"({PACKAGE_NAME})@({PACKAGE_VERSION})", identity)
                if match is None or not isinstance(diagnostic, str):
                    raise ValueError("malformed package warning")
                text = re.sub(r"\x1b\[[0-9;]*m", "", diagnostic)
                count = len(re.findall(r"^>\s+warning(?:\[[A-Za-z0-9_]+\])?:", text, re.MULTILINE))
                packages.append({"name": match[1], "version": match[2], "warning_count": count or None})
            reports.append({"id": report["id"], "packages": sorted(packages, key=lambda item: (item["name"], item["version"]))})
        return {"status": "available", "freshness": "unverified", "reports": sorted(reports, key=lambda item: item["id"])}
    except FileNotFoundError:
        return {"status": "unavailable", "reason": "no artifact at target/.future-incompat-report.json"}
    except (OSError, UnicodeError, ValueError, KeyError, TypeError, RecursionError):
        return {"status": "unavailable", "reason": "existing future-incompatibility artifact is unreadable, unsupported, or malformed"}


def run_qualification(root: Path, target: str | None, binary: Path | None) -> int:
    report = {"purpose": "existing development evidence, not an acceptance or distribution verdict"}
    failed = False
    try:
        manifest = qualification_file(root, "Cargo.toml")
        lock = qualification_file(root, "Cargo.lock")
        package = tomllib.loads(manifest.decode("utf-8"))["package"]
        schema = json.loads(qualification_file(root, "schemas/file-tile-config.schema.json"))
        version = schema["properties"]["schema_version"]["const"]
        if type(version) is not int or version < 1:
            raise ValueError("malformed schema identity")
        report["identity"] = {
            "status": "available",
            "package": qualification_token(package["name"], PACKAGE_NAME),
            "version": qualification_token(package["version"], PACKAGE_VERSION),
            "schema_format": qualification_token(schema["properties"]["format"]["const"], r"[A-Za-z0-9_.-]+"),
            "schema_version": version,
            "manifest_sha256": hashlib.sha256(manifest).hexdigest(),
            "lock_sha256": hashlib.sha256(lock).hexdigest(),
        }
    except (OSError, UnicodeError, ValueError, KeyError, TypeError, RecursionError):
        report["identity"] = {"status": "unavailable", "reason": "package, lockfile, or schema identity is unreadable or malformed"}
        failed = True
    try:
        pinned = tomllib.loads(qualification_file(root, "rust-toolchain.toml").decode("utf-8"))["toolchain"]["channel"]
        text = qualification_command(root, ["rustc", "-Vv"], "Rust compiler identity")
        fields = dict(line.split(": ", 1) for line in text.splitlines() if ": " in line)
        host = qualification_token(fields["host"], r"[A-Za-z0-9_]+(?:-[A-Za-z0-9_]+){2,}")
        report["toolchain"] = {
            "status": "available", "pinned": qualification_token(pinned, r"[A-Za-z0-9_.-]+"),
            "release": qualification_token(fields["release"], PACKAGE_VERSION), "host": host,
            "commit": qualification_token(fields["commit-hash"], r"[0-9a-f]{40}"),
        }
        target = target or host
    except (OSError, UnicodeError, ValueError, KeyError, TypeError):
        report["toolchain"] = {"status": "unavailable", "reason": "pinned or observed Rust identity is unavailable or malformed"}
        failed = True
    report["target"] = target
    try:
        if target is None:
            raise ValueError("target unavailable; supply --target or repair the Rust toolchain")
        text = qualification_command(root, ["cargo", "metadata", "--locked", "--offline", "--format-version", "1", "--filter-platform", target], "Cargo metadata")
        package, dependencies = qualification_dependencies(json.loads(text))
        identity = report["identity"]
        if identity["status"] == "available" and (package["name"], package["version"]) != (identity["package"], identity["version"]):
            raise ValueError("Cargo metadata root differs from the manifest identity")
        report["dependencies"] = {
            "status": "available", "scope": "target-filtered reachable graph, including build/proc-macro dependencies; not linked-binary contents",
            "root": package, "packages": dependencies,
        }
    except (ValueError, TypeError, RecursionError):
        report["dependencies"] = {"status": "unavailable", "reason": "locked/offline target metadata failed or returned malformed evidence; inspect Cargo metadata locally"}
        failed = True
    try:
        head = qualification_command(root, ["git", "rev-parse", "--verify", "HEAD"], "Git HEAD").strip()
        dirty = qualification_command(root, ["git", "--no-optional-locks", "status", "--porcelain=v1", "--untracked-files=normal"], "Git status")
        if any(re.match(r"[ MADRCU?!]{2} .+", line) is None for line in dirty.splitlines()):
            raise ValueError("malformed Git status")
        report["git"] = {"status": "available", "head": qualification_token(head, r"[0-9a-f]{40}|[0-9a-f]{64}"), "dirty": bool(dirty.strip())}
    except ValueError:
        report["git"] = {"status": "unavailable", "reason": "Git HEAD or working-tree status is unavailable"}
    report["future_incompatibilities"] = qualification_future(root)
    report["binary"] = {"status": "not_requested"}
    if binary is not None:
        try:
            if binary.is_symlink() or not binary.is_file():
                raise ValueError("not a regular file")
            with binary.open("rb") as source:
                digest = hashlib.file_digest(source, "sha256").hexdigest()
            report["binary"] = {"status": "available", "sha256": digest, "source_correspondence": "unverified"}
        except (OSError, ValueError):
            report["binary"] = {"status": "unavailable", "reason": "supplied binary is not a readable regular non-symlink file"}
            failed = True
    print(json.dumps(report, indent=2, sort_keys=True))
    return 1 if failed else 0


def create_native_fixture(parent: Path) -> Path:
    """Keep an owned private tree across launches. Never use production state."""
    root = Path(tempfile.mkdtemp(prefix="file-tile-workspace-test-", dir=parent))
    for folder in ("Alpha/One", "Beta/Two", "Empty folder"):
        (root / "folders" / folder).mkdir(parents=True, mode=0o700)
    (root / "folders/Alpha/example.txt").write_text("Synthetic file-tile fixture.\n", encoding="utf-8")
    (root / "folders/Alpha/preview.png").write_bytes(synthetic_png())
    (root / "folders/Alpha/binary.dat").write_bytes(b"Synthetic binary fixture\x00\xff")
    (root / "state").mkdir(mode=0o700)
    return root


def synthetic_png() -> bytes:
    """A small owned image, generated without a decoder or personal source file."""
    def chunk(kind: bytes, payload: bytes) -> bytes:
        return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload) & 0xffffffff)

    width = height = 32
    pixels = b"".join(b"\x00" + bytes((108, 136, 121)) * width for _ in range(height))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(pixels)) + chunk(b"IEND", b""))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("docs", help="validate local Markdown references")
    commands.add_parser("guard", help="reject textual Rust suppression/ignore attributes")
    commands.add_parser("fixture", help="create reusable private folders and persistence state")
    focused = commands.add_parser("focused", help="run a Rust filter and reject zero executed tests")
    focused.add_argument("--test", required=True, type=nonempty)
    focused.add_argument("--release", action="store_true")
    qualification = commands.add_parser("qualification", help="report existing development evidence without building or launching")
    qualification.add_argument("--target", type=target_triple, help="Cargo target triple; defaults to the observed Rust host")
    qualification.add_argument("--binary", type=Path, help="hash only this explicitly supplied existing binary")
    arguments = parser.parse_args()
    try:
        if arguments.command == "docs":
            return report_check(*check_documents(ROOT), "documentation")
        if arguments.command == "guard":
            return report_check(*check_sources(ROOT), "Rust source")
        if arguments.command == "qualification":
            return run_qualification(ROOT, arguments.target, arguments.binary)
        if arguments.command == "fixture":
            root = create_native_fixture(Path(tempfile.gettempdir()))
            print("Reuse this exact command after closing to test restoration:")
            print(shlex.join(["task", "run:isolated", f"STATE_DIR={root / 'state'}", "--", "--directory", str(root / "folders")]))
            print("Use the red close button or Cmd+Q to finish saving before exit; a failed save keeps the window open.")
            print("Alpha contains synthetic text, PNG, and binary preview fixtures. No personal files are needed.")
            return 0
        return run_focused(ROOT, arguments.test, arguments.release)
    except KeyboardInterrupt:
        print("Development check interrupted; no successful outcome recorded.", file=sys.stderr)
        return 130
    except (OSError, UnicodeError, ValueError, subprocess.SubprocessError) as error:
        if arguments.command == "qualification":
            print("Qualification report failed. Check the local evidence inputs and rerun.", file=sys.stderr)
            return 1
        print(f"Development check failed: {error}. Fix the input or environment and rerun.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
