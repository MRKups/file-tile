"""Generate a concise, deduplicated LICENSES/THIRD-PARTY-NOTICES.txt for macOS builds."""

from __future__ import annotations

import json
from pathlib import Path
import re
import subprocess


ROOT = Path(__file__).resolve().parent.parent
TARGET = "aarch64-apple-darwin"
OUTPUT = ROOT / "LICENSES" / "THIRD-PARTY-NOTICES.txt"
LICENSE_PREFIXES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT", "UNLICENSE")
COPYRIGHT_LINE = re.compile(r"(?i)^.*(?:copyright|\(c\)|©).*$")

ELECTED_LICENSES = {
    "self_cell": "Apache-2.0 (elected from Apache-2.0 OR GPL-2.0-only)",
    "objc2-app-kit": "MIT (elected from Zlib OR Apache-2.0 OR MIT)",
    "objc2-core-foundation": "MIT (elected from Zlib OR Apache-2.0 OR MIT)",
    "objc2-core-graphics": "MIT (elected from Zlib OR Apache-2.0 OR MIT)",
    "objc2-core-services": "MIT (elected from Zlib OR Apache-2.0 OR MIT)",
    "objc2-quartz-core": "MIT (elected from Zlib OR Apache-2.0 OR MIT)",
    "dispatch2": "MIT (elected from Zlib OR Apache-2.0 OR MIT)",
}

# Crates with unique BSD/ISC/Zlib clauses whose exact upstream notice is printed verbatim.
VERBATIM_KEYWORDS = ("BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib")


def collect_packages() -> list[dict]:
    raw = subprocess.check_output(
        [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--filter-platform",
            TARGET,
        ],
        cwd=ROOT,
        text=True,
    )
    metadata = json.loads(raw)
    packages = {pkg["id"]: pkg for pkg in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    workspace_members = set(metadata["workspace_members"])

    visited: set[str] = set()
    stack = list(workspace_members)
    while stack:
        current = stack.pop()
        if current in visited:
            continue
        visited.add(current)
        for dep in nodes[current].get("deps", []):
            kinds = [entry.get("kind") for entry in dep.get("dep_kinds", [])]
            if None in kinds:
                stack.append(dep["pkg"])

    external = [
        packages[pkg_id]
        for pkg_id in visited
        if pkg_id not in workspace_members and packages[pkg_id].get("source") is not None
    ]
    return sorted(external, key=lambda pkg: (pkg["name"], pkg["version"]))


def find_license_files(crate_name: str, crate_dir: Path) -> list[Path]:
    files = []
    for entry in sorted(crate_dir.iterdir(), key=lambda p: p.name):
        if not entry.is_file():
            continue
        upper = entry.name.upper()
        if not upper.startswith(LICENSE_PREFIXES):
            continue
        if crate_name == "self_cell" and "GPL" in upper:
            continue
        files.append(entry)
    return files


def extract_copyrights(files: list[Path], fallback: str) -> list[str]:
    seen: list[str] = []
    for lic in files:
        for raw_line in lic.read_text(encoding="utf-8", errors="replace").splitlines()[:40]:
            line = " ".join(raw_line.strip().split())
            if not line or len(line) > 140:
                continue
            lowered = line.lower()
            if any(
                phrase in lowered
                for phrase in (
                    "copyright notice and this permission",
                    "above copyright notice",
                    "endorse or promote",
                    "copyright owner",
                    "copyright license",
                    "[yyyy]",
                )
            ):
                continue
            if COPYRIGHT_LINE.match(line) and line not in seen:
                seen.append(line)
    return seen or [f"Copyright (c) {fallback}"]


def canonical_text(packages: list[dict], match_name: str, match_file: str) -> str:
    for pkg in packages:
        if pkg["name"] == match_name:
            path = Path(pkg["manifest_path"]).parent / match_file
            if path.is_file():
                return path.read_text(encoding="utf-8", errors="replace").strip()
    raise RuntimeError(f"Missing canonical license file {match_name}/{match_file}")


def main() -> int:
    packages = collect_packages()
    lines = [
        "file-tile Third-Party Software Notices",
        "======================================",
        "",
        f"Target platform: {TARGET}",
        f"Bundled framework: Sparkle 2 (MIT License, https://github.com/sparkle-project/Sparkle)",
        f"Linked Rust packages: {len(packages)}",
        "",
        "Bundled font licenses (SIL Open Font License 1.1):",
        "- LICENSES/Cantarell-OFL.txt",
        "- LICENSES/Monaspace-Argon-OFL.txt",
        "",
        "1. Package Index and Copyright Notices",
        "--------------------------------------",
        "",
        "- Sparkle 2.9.6 | MIT | https://github.com/sparkle-project/Sparkle",
        "  Copyright (c) 2006-2013 Andy Matuschak; 2009-2013 Elgato Systems GmbH; 2011-2014 Kornel Lesinski; 2015-2017 Mayur Pawashe; 2014 C.W. Betts; 2014 Petroules Corporation; 2014 Big Nerd Ranch.",
    ]

    verbatim_packages: list[tuple[str, str, str, list[Path]]] = []
    for pkg in packages:
        name = pkg["name"]
        version = pkg["version"]
        declared = ELECTED_LICENSES.get(name, pkg.get("license") or "Unknown")
        repo = pkg.get("repository") or ""
        authors = ", ".join(pkg.get("authors") or []) or f"The {name} contributors"
        crate_dir = Path(pkg["manifest_path"]).parent
        files = find_license_files(name, crate_dir)
        copyrights = extract_copyrights(files, authors)

        repo_part = f" | {repo}" if repo else ""
        lines.append(f"- {name} {version} | {declared}{repo_part}")
        for c_line in copyrights[:3]:
            lines.append(f"  {c_line}")

        if files and any(keyword in declared for keyword in VERBATIM_KEYWORDS):
            bsd_files = [
                f for f in files if "APACHE" not in f.name.upper() and "MIT" not in f.name.upper()
            ] or files
            verbatim_packages.append((name, version, declared, bsd_files))

    lines.extend(["", "2. Verbatim BSD, ISC, and Zlib Package Notices", "----------------------------------------------"])
    for name, version, declared, files in verbatim_packages:
        lines.extend(["", f"=== {name} {version} ({declared}) ==="])
        for lic in files:
            lines.append(lic.read_text(encoding="utf-8", errors="replace").strip())

    lines.extend(["", "3. Shared Standard License Texts", "--------------------------------"])
    shared = [
        ("MIT License", canonical_text(packages, "iced", "LICENSE")),
        ("Apache License, Version 2.0", canonical_text(packages, "serde", "LICENSE-APACHE")),
        ("Boost Software License 1.0", canonical_text(packages, "ryu", "LICENSE-BOOST") if any(p["name"] == "ryu" for p in packages) else ""),
        ("Unicode License v3 (Unicode-3.0)", canonical_text(packages, "icu_normalizer_data", "LICENSE-UNICODE") if any(p["name"] == "icu_normalizer_data" for p in packages) else ""),
        ("Creative Commons CC0 1.0 Universal", canonical_text(packages, "notify", "LICENSE-CC0") if any(p["name"] == "notify" for p in packages) else ""),
    ]
    for title, body in shared:
        if body:
            lines.extend(["", f"=== {title} ===", body])

    lines.append("")
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text("\n".join(lines), encoding="utf-8")
    print(f"Wrote {OUTPUT.relative_to(ROOT)} ({OUTPUT.stat().st_size // 1024} KiB, {len(packages)} packages).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
