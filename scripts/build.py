"""Build with machine-independent compiler source locations for release artifacts."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import platform
import subprocess
import sys


ROOT = Path(__file__).resolve().parent.parent


def release_environment() -> dict[str, str]:
    environment = os.environ.copy()
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
    if sys.platform == "darwin" and platform.machine() != "arm64":
        print("file-tile targets Apple Silicon (arm64) only.", file=sys.stderr)
        return 1
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("profile")
    parser.add_argument("arguments", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    environment = release_environment() if args.profile == "release" else os.environ.copy()
    return subprocess.run(
        ["cargo", "build", "--locked", "--profile", args.profile, *args.arguments],
        cwd=ROOT,
        env=environment,
        check=False,
    ).returncode


if __name__ == "__main__":
    raise SystemExit(main())
