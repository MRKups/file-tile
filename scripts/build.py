"""Build with machine-independent compiler source locations for release artifacts."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import tomllib


ROOT = Path(__file__).resolve().parent.parent


def sync_lockfile_version(root: Path, version: str) -> None:
    lock_path = root / "Cargo.lock"
    if not lock_path.is_file():
        return
    text = lock_path.read_text(encoding="utf-8")
    pattern = re.compile(
        r'(\[\[package\]\]\nname = "file-tile(?:-[a-z0-9-]+)?"\nversion = )"([^"]+)"(\n(?!source = ))'
    )
    updated = pattern.sub(rf'\g<1>"{version}"\g<3>', text)
    if updated != text:
        lock_path.write_text(updated, encoding="utf-8")


def workspace_version(root: Path = ROOT) -> str:
    with (root / "Cargo.toml").open("rb") as handle:
        cargo = tomllib.load(handle)
    package = cargo.get("package", {})
    version = package.get("version") if isinstance(package, dict) else None
    if isinstance(version, dict) and version.get("workspace") is True:
        workspace = cargo.get("workspace", {})
        if isinstance(workspace, dict) and isinstance(workspace.get("package"), dict):
            version = workspace["package"].get("version")
    if not isinstance(version, str) or not version.strip():
        raise SystemExit("Missing package version in Cargo.toml")
    return version.strip()


def git_commit(root: Path = ROOT) -> str:
    explicit = os.environ.get("FILE_TILE_GIT_COMMIT", "").strip()
    if explicit:
        return explicit
    try:
        sha = subprocess.check_output(
            ["git", "--no-optional-locks", "rev-parse", "--short=7", "HEAD"],
            cwd=root,
            stderr=subprocess.DEVNULL,
            text=True,
        ).strip()
        if not sha:
            return "unknown"
        modified = subprocess.check_output(
            [
                "git",
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "--untracked-files=normal",
            ],
            cwd=root,
            stderr=subprocess.DEVNULL,
            text=True,
        ).strip()
        return f"{sha}+modified" if modified else sha
    except (OSError, subprocess.SubprocessError):
        return "unknown"


def release_environment() -> dict[str, str]:
    environment = os.environ.copy()
    environment.setdefault("FILE_TILE_GIT_COMMIT", git_commit())
    if sys.platform == "darwin":
        environment.setdefault("MACOSX_DEPLOYMENT_TARGET", "15.0")
    encoded = environment.get("CARGO_ENCODED_RUSTFLAGS")
    flags = encoded.split("\x1f") if encoded is not None else environment.get("RUSTFLAGS", "").split()
    home = Path.home()
    prefixes = {
        home: "/build/home",
        Path(environment.get("CARGO_HOME", home / ".cargo")): "/build/cargo",
        ROOT: "/build/file-tile",
    }
    if target := environment.get("CARGO_TARGET_DIR"):
        prefixes[Path(target).absolute()] = "/build/target"
    # Remapping changes compiler diagnostics and file!() locations, not runtime
    # filesystem paths. Include both spellings when a build root is a symlink.
    mappings = {}
    for path, replacement in prefixes.items():
        mappings[str(path.absolute())] = replacement
        mappings[str(path.resolve())] = replacement
    for source, replacement in sorted(mappings.items(), key=lambda item: len(item[0])):
        flags.append(f"--remap-path-prefix={source}={replacement}")
    # Cargo's encoded form preserves spaces in checkout and home-folder names.
    environment["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
    return environment


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--print-version",
        action="store_true",
        help="Print the single workspace version from Cargo.toml and exit",
    )
    parser.add_argument(
        "--print-commit",
        action="store_true",
        help="Print the resolved Git commit identifier (<short-sha>[+modified]) and exit",
    )
    parser.add_argument("profile", nargs="?")
    parser.add_argument("arguments", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.print_version:
        print(workspace_version())
        return 0
    if args.print_commit:
        print(git_commit())
        return 0
    if not args.profile:
        parser.error("the following arguments are required: profile")
    if sys.platform == "darwin" and platform.machine() != "arm64":
        print("file-tile targets Apple Silicon (arm64) only.", file=sys.stderr)
        return 1
    sync_lockfile_version(ROOT, workspace_version(ROOT))
    if args.profile == "release":
        environment = release_environment()
    else:
        environment = os.environ.copy()
        environment.setdefault("FILE_TILE_GIT_COMMIT", git_commit())
    return subprocess.run(
        ["cargo", "build", "--locked", "--profile", args.profile, *args.arguments],
        cwd=ROOT,
        env=environment,
        check=False,
    ).returncode


if __name__ == "__main__":
    raise SystemExit(main())
