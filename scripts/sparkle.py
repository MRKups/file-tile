#!/usr/bin/env python3
"""Fetch, stage, keygen, and appcast helper for Sparkle 2 macOS releases."""

from __future__ import annotations

import argparse
import hashlib
import os
import plistlib
import shutil
import subprocess
import sys
import tempfile
import tomllib
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CARGO_TOML = ROOT / "Cargo.toml"
SPARKLE_TOML = ROOT / "packaging" / "sparkle.toml"
CACHE_ROOT = ROOT / "target" / "sparkle"
STAGED_FRAMEWORK = CACHE_ROOT / "Sparkle.framework"
STAGED_PLIST = CACHE_ROOT / "Info.sparkle.plist"
STAGED_ENTITLEMENTS = CACHE_ROOT / "Entitlements.plist"


def resolve_package_field(cargo: dict[str, object], key: str) -> str:
    package = cargo.get("package")
    if not isinstance(package, dict):
        raise SystemExit("Invalid [package] table in Cargo.toml")
    value = package.get(key)
    if isinstance(value, dict) and value.get("workspace") is True:
        workspace = cargo.get("workspace")
        if isinstance(workspace, dict) and isinstance(workspace.get("package"), dict):
            value = workspace["package"].get(key)
    if not isinstance(value, str) or not value.strip():
        raise SystemExit(f"Missing or invalid package.{key} in Cargo.toml")
    return value.strip()


def resolve_git_commit() -> str:
    explicit = os.environ.get("FILE_TILE_GIT_COMMIT", "").strip()
    if explicit:
        return explicit
    try:
        sha = subprocess.check_output(
            ["git", "--no-optional-locks", "rev-parse", "--short=7", "HEAD"],
            cwd=ROOT,
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
            cwd=ROOT,
            stderr=subprocess.DEVNULL,
            text=True,
        ).strip()
        return f"{sha}+modified" if modified else sha
    except (OSError, subprocess.SubprocessError):
        return "unknown"


def load_config() -> tuple[str, str, dict[str, object]]:
    with CARGO_TOML.open("rb") as handle:
        cargo = tomllib.load(handle)
    app_version = resolve_package_field(cargo, "version")
    repo_url = resolve_package_field(cargo, "repository").rstrip("/")
    with SPARKLE_TOML.open("rb") as handle:
        sparkle_cfg = tomllib.load(handle)
    return app_version, repo_url, sparkle_cfg


def sha256_file(path: Path) -> str:
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def ensure_sparkle_release(sparkle_cfg: dict[str, object]) -> Path:
    meta = sparkle_cfg["sparkle"]
    if not isinstance(meta, dict):
        raise SystemExit("Invalid [sparkle] table in packaging/sparkle.toml")
    version = str(meta["version"])
    url = str(meta["url"])
    expected_sha256 = str(meta["sha256"]).lower()

    CACHE_ROOT.mkdir(parents=True, exist_ok=True)
    archive = CACHE_ROOT / f"Sparkle-{version}.tar.xz"
    extract_dir = CACHE_ROOT / version
    framework = extract_dir / "Sparkle.framework"
    keygen = extract_dir / "bin" / "generate_keys"
    appcast_bin = extract_dir / "bin" / "generate_appcast"

    if framework.is_dir() and keygen.is_file() and appcast_bin.is_file():
        return extract_dir

    if not archive.is_file() or sha256_file(archive) != expected_sha256:
        tmp_archive = CACHE_ROOT / f"Sparkle-{version}.tar.xz.part"
        print(f"Downloading Sparkle {version} from {url}")
        with urllib.request.urlopen(url, timeout=60) as response, tmp_archive.open("wb") as out:
            shutil.copyfileobj(response, out)
        actual_sha256 = sha256_file(tmp_archive)
        if actual_sha256 != expected_sha256:
            tmp_archive.unlink(missing_ok=True)
            raise SystemExit(
                f"Sparkle archive SHA-256 mismatch: expected {expected_sha256}, got {actual_sha256}"
            )
        tmp_archive.replace(archive)

    if extract_dir.exists():
        shutil.rmtree(extract_dir)
    extract_dir.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["/usr/bin/tar", "-xJf", str(archive), "-C", str(extract_dir)],
        check=True,
    )
    if not framework.is_dir():
        raise SystemExit(f"Expected Sparkle.framework inside {extract_dir}")
    return extract_dir


def remove_tree_or_link(path: Path) -> None:
    if path.is_symlink() or path.is_file():
        path.unlink()
    elif path.exists():
        shutil.rmtree(path)


def cmd_fetch(_args: argparse.Namespace) -> int:
    _, _, sparkle_cfg = load_config()
    extract_dir = ensure_sparkle_release(sparkle_cfg)
    print(f"Sparkle ready at {extract_dir}")
    return 0


def cmd_prepare(_args: argparse.Namespace) -> int:
    app_version, _, sparkle_cfg = load_config()
    extract_dir = ensure_sparkle_release(sparkle_cfg)
    source_framework = extract_dir / "Sparkle.framework"

    remove_tree_or_link(STAGED_FRAMEWORK)
    shutil.copytree(source_framework, STAGED_FRAMEWORK, symlinks=True)

    # file-tile runs under Hardened Runtime without App Sandbox; prune sandbox-only XPC bundles
    # and strip Intel x86_64 slices so the packaged .app is pure arm64.
    for xpc_rel in (Path("XPCServices"), Path("Versions/B/XPCServices")):
        remove_tree_or_link(STAGED_FRAMEWORK / xpc_rel)
    for mach_rel in (
        Path("Versions/B/Sparkle"),
        Path("Versions/B/Autoupdate"),
        Path("Versions/B/Updater.app/Contents/MacOS/Updater"),
    ):
        mach_path = STAGED_FRAMEWORK / mach_rel
        if mach_path.is_file() and not mach_path.is_symlink():
            archs = subprocess.check_output(
                ["/usr/bin/lipo", "-archs", str(mach_path)], text=True
            ).split()
            if "arm64" in archs and len(archs) > 1:
                thinned = mach_path.with_suffix(".arm64")
                subprocess.run(
                    ["/usr/bin/lipo", "-thin", "arm64", str(mach_path), "-output", str(thinned)],
                    check=True,
                )
                thinned.replace(mach_path)

    feed_url = os.environ.get("SPARKLE_FEED_URL", "").strip() or str(
        sparkle_cfg.get("feed_url", "")
    ).strip()
    if not feed_url:
        raise SystemExit("feed_url in packaging/sparkle.toml cannot be empty")

    public_key = os.environ.get("SPARKLE_PUBLIC_ED_KEY", "").strip() or str(
        sparkle_cfg.get("public_ed_key", "")
    ).strip()
    auto_checks = bool(sparkle_cfg.get("automatic_checks_default", False))

    git_commit = resolve_git_commit()
    plist_payload: dict[str, object] = {
        "CFBundleShortVersionString": app_version,
        "CFBundleVersion": app_version,
        "FTGitCommit": git_commit,
        "SUFeedURL": feed_url,
        "SUEnableAutomaticChecks": auto_checks,
        "NSDesktopFolderUsageDescription": "file-tile needs access to browse files in your Desktop folder.",
        "NSDocumentsFolderUsageDescription": "file-tile needs access to browse files in your Documents folder.",
        "NSDownloadsFolderUsageDescription": "file-tile needs access to browse files in your Downloads folder.",
    }
    if public_key:
        plist_payload["SUPublicEDKey"] = public_key

    STAGED_PLIST.write_bytes(plistlib.dumps(plist_payload, fmt=plistlib.FMT_XML))

    signing_identity = (
        os.environ.get("RAW_APPLE_SIGNING_IDENTITY", "").strip()
        or os.environ.get("APPLE_SIGNING_IDENTITY", "").strip()
    )
    entitlements_payload: dict[str, object] = {}
    if not signing_identity or signing_identity == "-":
        # Ad-hoc Hardened Runtime builds have no TeamIdentifier; disable library validation
        # on ad-hoc bundles so macOS amfid permits loading Contents/Frameworks/Sparkle.framework.
        entitlements_payload["com.apple.security.cs.disable-library-validation"] = True
    STAGED_ENTITLEMENTS.write_bytes(
        plistlib.dumps(entitlements_payload, fmt=plistlib.FMT_XML)
    )

    key_status = "configured" if public_key else "empty (awaiting SUPublicEDKey)"
    print(f"Staged Sparkle.framework and Info.sparkle.plist (version {app_version}, key: {key_status})")
    return 0


def cmd_keys(args: argparse.Namespace) -> int:
    _, _, sparkle_cfg = load_config()
    extract_dir = ensure_sparkle_release(sparkle_cfg)
    keygen = extract_dir / "bin" / "generate_keys"
    extra = list(args.extra)
    if extra and extra[0] == "--":
        extra = extra[1:]
    completed = subprocess.run([str(keygen), *extra], check=False)
    return completed.returncode


def cmd_appcast(args: argparse.Namespace) -> int:
    app_version, repo_url, sparkle_cfg = load_config()
    private_key = os.environ.get("SPARKLE_PRIVATE_KEY", "").strip()
    if not private_key:
        raise SystemExit(
            "Cannot generate appcast.xml: SPARKLE_PRIVATE_KEY environment variable is empty."
        )

    dist_dir = Path(args.dir).resolve()
    if not dist_dir.is_dir():
        raise SystemExit(f"Distribution directory does not exist: {dist_dir}")

    dmg_files = sorted(dist_dir.glob("*.dmg"))
    if len(dmg_files) != 1:
        raise SystemExit(
            f"Expected exactly 1 .dmg archive in {dist_dir}, found {len(dmg_files)}"
        )

    prefix = args.download_url_prefix or f"{repo_url}/releases/download/v{app_version}/"
    if not prefix.endswith("/"):
        prefix += "/"

    extract_dir = ensure_sparkle_release(sparkle_cfg)
    generate_appcast = extract_dir / "bin" / "generate_appcast"

    with tempfile.TemporaryDirectory(prefix="file-tile-appcast-") as tmp:
        stage = Path(tmp)
        staged_dmg = stage / dmg_files[0].name
        shutil.copy2(dmg_files[0], staged_dmg)
        subprocess.run(
            [
                str(generate_appcast),
                "--ed-key-file",
                "-",
                "--download-url-prefix",
                prefix,
                str(stage),
            ],
            input=(private_key + "\n").encode("utf-8"),
            check=True,
        )
        generated = stage / "appcast.xml"
        if not generated.is_file():
            raise SystemExit("generate_appcast completed without producing appcast.xml")
        destination = dist_dir / "appcast.xml"
        shutil.copy2(generated, destination)
        print(f"Wrote {destination}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    fetch_p = sub.add_parser("fetch", help="Download and verify pinned Sparkle release")
    fetch_p.set_defaults(func=cmd_fetch)

    prep_p = sub.add_parser(
        "prepare",
        help="Stage Sparkle.framework and Info.sparkle.plist for cargo-packager",
    )
    prep_p.set_defaults(func=cmd_prepare)

    keys_p = sub.add_parser("keys", help="Run Sparkle generate_keys tool")
    keys_p.add_argument("extra", nargs=argparse.REMAINDER)
    keys_p.set_defaults(func=cmd_keys)

    appcast_p = sub.add_parser("appcast", help="Generate signed appcast.xml for a .dmg release")
    appcast_p.add_argument("--dir", required=True, help="Directory containing the release .dmg")
    appcast_p.add_argument(
        "--download-url-prefix",
        default="",
        help="Release asset URL prefix (defaults to GitHub Releases v<Cargo.toml version>/)",
    )
    appcast_p.set_defaults(func=cmd_appcast)

    args = parser.parse_args()
    return int(args.func(args))


if __name__ == "__main__":
    sys.exit(main())
