"""Behavioral checks for the scaffold's development tooling."""

import copy
from contextlib import redirect_stdout
import hashlib
import io
import json
import marshal
from pathlib import Path
import re
import shlex
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import dev


class NativeFixtureTests(unittest.TestCase):
    def test_fixture_instructions_use_the_close_barrier_without_a_hidden_status_row(self):
        with tempfile.TemporaryDirectory(prefix="file-tile-tooling-") as temporary:
            root = Path(temporary) / "owned spaces 'quote\" $HOME $(owned); & [*]"
            output = io.StringIO()
            with patch("dev.create_native_fixture", return_value=root), patch.object(sys, "argv", ["dev.py", "fixture"]), redirect_stdout(output):
                self.assertEqual(dev.main(), 0)
            text = output.getvalue()
            commands = [line for line in text.splitlines() if line.startswith("task ")]
            self.assertEqual(len(commands), 1)
            arguments = shlex.split(commands[0])
            self.assertEqual(arguments, [
                "task", "run:isolated", f"STATE_DIR={root / 'state'}",
                "--", "--directory", str(root / "folders"),
            ])
            # Shell substitutions must stay literal, not merely survive shlex.split.
            self.assertEqual(commands[0], shlex.join(arguments))

            taskfile = (dev.ROOT / "Taskfile.yml").read_text(encoding="utf-8")
            isolated = re.search(r"(?ms)^  run:isolated:\n(.*?)(?=^  \S|\Z)", taskfile)
            self.assertIsNotNone(isolated)
            self.assertIn("vars: [STATE_DIR]", isolated[1])
            templates = [line.removeprefix("      - ") for line in isolated[1].splitlines()
                         if line.startswith("      - ")]
            self.assertEqual(len(templates), 1)
            # Check the existing Task forwarding template without running Task or Cargo.
            forwarded = templates[0].removeprefix("'").removesuffix("'")
            forwarded = forwarded.replace("{{.PROFILE | q}}", shlex.quote("dev"))
            forwarded = forwarded.replace("{{.STATE_DIR | q}}", shlex.quote(arguments[2].split("=", 1)[1]))
            forwarded = forwarded.replace("{{.CLI_ARGS}}", shlex.join(arguments[4:]))
            self.assertEqual(shlex.split(forwarded), [
                "cargo", "run", "--locked", "--profile", "dev", "--",
                "--state-dir", str(root / "state"), "--directory", str(root / "folders"),
            ])
            self.assertIn("red close button", text)

    def test_private_fixture_separates_state_and_synthetic_folders_and_is_retained(self):
        with tempfile.TemporaryDirectory(prefix="file-tile-tooling-") as temporary:
            root = dev.create_native_fixture(Path(temporary))
            self.assertTrue(root.is_relative_to(Path(temporary)))
            self.assertEqual(root.stat().st_mode & 0o777, 0o700)
            self.assertTrue((root / "state").is_dir())
            self.assertEqual(list((root / "state").iterdir()), [])
            self.assertTrue((root / "folders/Alpha/One").is_dir())
            self.assertTrue((root / "folders/Beta/Two").is_dir())
            self.assertIn("Synthetic", (root / "folders/Alpha/example.txt").read_text())
            image = (root / "folders/Alpha/preview.png").read_bytes()
            self.assertTrue(image.startswith(b"\x89PNG\r\n\x1a\n"))
            self.assertEqual(struct.unpack(">II", image[16:24]), (32, 32))
            self.assertIn(b"\x00\xff", (root / "folders/Alpha/binary.dat").read_bytes())
            self.assertTrue(root.exists())

    def test_fixture_does_not_reuse_or_change_an_existing_tree(self):
        with tempfile.TemporaryDirectory(prefix="file-tile-tooling-") as temporary:
            parent = Path(temporary)
            first = dev.create_native_fixture(parent)
            (first / "state/preserve").write_text("owned fixture state")
            second = dev.create_native_fixture(parent)
            self.assertNotEqual(first, second)
            self.assertEqual((first / "state/preserve").read_text(), "owned fixture state")


class QualificationReportTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="file-tile-qualification-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / "Cargo.toml").write_text('[package]\nname = "file-tile"\nversion = "0.1.0"\n', encoding="utf-8")
        (self.root / "Cargo.lock").write_text("# Synthetic lock evidence\nversion = 4\n", encoding="utf-8")
        (self.root / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.99.0"\n', encoding="utf-8")
        (self.root / "schemas").mkdir()
        (self.root / "schemas/file-tile-config.schema.json").write_text(json.dumps({"properties": {
            "format": {"const": "file-tile.config"}, "schema_version": {"const": 7},
        }}), encoding="utf-8")
        self.sentinel = "/private/qualification-sentinel/personal-data"
        self.metadata = {
            "packages": [
                {"id": identity, "name": name, "version": version, "license": license,
                 "license_file": self.sentinel if identity == "old" else None,
                 "manifest_path": self.sentinel, "source": self.sentinel,
                 "description": self.sentinel, "targets": [{"kind": ["proc-macro"]}]}
                for identity, name, version, license in (
                    ("root", "file-tile", "0.1.0", None),
                    ("old", "shared", "1.0.0", None),
                    ("new", "shared", "2.0.0", "MIT/Apache-2.0"),
                    ("build", "build-helper", "1.2.3", "Apache-2.0 OR GPL-2.0-only"),
                    ("inactive", "not-for-target", "3.0.0", "MIT"),
                )
            ],
            "resolve": {"root": "root", "nodes": [
                {"id": "root", "features": ["svg"], "deps": [
                    {"pkg": "new", "dep_kinds": [{"kind": None, "target": None}]},
                    {"pkg": "build", "dep_kinds": [{"kind": "build", "target": None}]},
                ]},
                {"id": "new", "features": ["std", "default"], "deps": [{"pkg": "old"}]},
                {"id": "old", "features": [], "deps": [{"pkg": "new"}]},
                {"id": "build", "features": [], "deps": []},
                {"id": "inactive", "features": [], "deps": []},
            ]},
            "target_directory": self.sentinel, "workspace_root": self.sentinel,
        }
        self.commands = []

    def command(self, command, *, cwd, merge_stderr):
        self.assertEqual(cwd, self.root)
        self.assertFalse(merge_stderr)
        self.commands.append(command)
        if command == ["rustc", "-Vv"]:
            text = f"rustc 1.99.0\nrelease: 1.99.0\nhost: aarch64-apple-darwin\ncommit-hash: {'b' * 40}\nignored: {self.sentinel}\n"
        elif command[:2] == ["cargo", "metadata"]:
            text = json.dumps(self.metadata)
        elif command[:2] == ["git", "rev-parse"]:
            text = "a" * 40 + "\n"
        elif command[:3] == ["git", "--no-optional-locks", "status"]:
            text = f"?? {self.sentinel}\n"
        else:
            self.fail(f"Unexpected command: {command}")
        return subprocess.CompletedProcess(command, 0, stdout=text)

    def collect(self, target=None, binary=None, command=None):
        output = io.StringIO()
        with patch("dev.run_command", side_effect=command or self.command), redirect_stdout(output):
            status = dev.run_qualification(self.root, target, binary)
        self.assertNotIn(self.sentinel, output.getvalue())
        self.assertNotIn(str(self.root), output.getvalue())
        return status, json.loads(output.getvalue())

    def test_metadata_failures_and_malformed_responses_do_not_expose_diagnostics(self):
        private_license = copy.deepcopy(self.metadata)
        private_license["packages"][1]["license"] = self.sentinel
        for result in (
            subprocess.CompletedProcess([], 101, stdout=self.sentinel),
            subprocess.CompletedProcess([], 0, stdout="not JSON " + self.sentinel),
            subprocess.CompletedProcess([], 0, stdout="[]"),
            subprocess.CompletedProcess([], 0, stdout="[" * 2000 + "]" * 2000),
            subprocess.CompletedProcess([], 0, stdout=None),
            subprocess.CompletedProcess([], 0, stdout=json.dumps(private_license)),
            FileNotFoundError(self.sentinel),
        ):
            with self.subTest(result=result):
                def command(arguments, *, cwd, merge_stderr):
                    if arguments[:2] == ["cargo", "metadata"]:
                        if isinstance(result, Exception):
                            raise result
                        return result
                    return self.command(arguments, cwd=cwd, merge_stderr=merge_stderr)
                status, report = self.collect(command=command)
                self.assertEqual(status, 1)
                self.assertEqual(report["dependencies"]["status"], "unavailable")
                self.assertNotIn("packages", report["dependencies"])

    def test_existing_warning_summary_never_publishes_diagnostic_text(self):
        (self.root / "target").mkdir()
        path = self.root / "target/.future-incompat-report.json"
        path.write_text(json.dumps({"version": 0, "reports": [{
            "id": 1, "suggestion_message": self.sentinel, "per_package": {
                "block@0.1.6": f"> \u001b[33mwarning\u001b[0m: static of uninhabited type\n> --> {self.sentinel}\n",
            },
        }]}), encoding="utf-8")
        status, report = self.collect()
        self.assertEqual(status, 0)
        self.assertEqual(report["future_incompatibilities"], {
            "status": "available", "freshness": "unverified", "reports": [
                {"id": 1, "packages": [{"name": "block", "version": "0.1.6", "warning_count": 1}]},
            ],
        })
        for contents in ("not JSON " + self.sentinel, '{"version":1,"reports":[]}', '{"version":0,"reports":[{}]}'):
            path.write_text(contents, encoding="utf-8")
            status, report = self.collect()
            self.assertEqual(status, 0)
            self.assertEqual(report["future_incompatibilities"]["status"], "unavailable")

    def test_subprocess_stderr_cannot_corrupt_json_or_leak_into_the_report(self):
        source = f'import sys\nsys.stderr.write({self.sentinel!r})\nprint("{{}}")\n'
        returned = self.root / "returned-output.json"
        # An owned wrapper captures Python output and inherited child descriptors.
        # Carry the current two callables so in-memory mutations exercise this boundary too.
        wrapper = (
            'import marshal, sys\nfrom pathlib import Path\n'
            f'sys.path.insert(0, {str(Path(dev.__file__).resolve().parent)!r})\n'
            'import dev\n'
            f'dev.qualification_command.__code__ = marshal.loads({marshal.dumps(dev.qualification_command.__code__)!r})\n'
            f'dev.run_command.__code__ = marshal.loads({marshal.dumps(dev.run_command.__code__)!r})\n'
            f'output = dev.qualification_command(Path({str(self.root)!r}), '
            f'[sys.executable, "-c", {source!r}], "Synthetic evidence")\n'
            f'Path({str(returned)!r}).write_text(output, encoding="utf-8")\n'
        )
        result = subprocess.run(
            [sys.executable, "-B", "-c", wrapper], cwd=self.root,
            capture_output=True, text=True, check=False, timeout=10,
        )
        self.assertNotIn(self.sentinel, result.stdout)
        self.assertNotIn(self.sentinel, result.stderr)
        self.assertEqual(result.returncode, 0, result.stderr)
        output = returned.read_text(encoding="utf-8")
        self.assertNotIn(self.sentinel, output)
        self.assertEqual(json.loads(output), {})

    def test_binary_is_only_hashed_when_supplied_and_never_executed(self):
        binary = self.root / "existing-binary"
        contents = b"Synthetic binary evidence, never executed\x00\xff"
        binary.write_bytes(contents)
        status, report = self.collect(binary=binary)
        self.assertEqual(status, 0)
        self.assertEqual(report["binary"], {
            "status": "available", "sha256": hashlib.sha256(contents).hexdigest(),
            "source_correspondence": "unverified",
        })
        self.assertNotIn(str(binary), json.dumps(report))
        for path in (self.root, self.root / "absent"):
            status, report = self.collect(binary=path)
            self.assertEqual(status, 1)
            self.assertEqual(report["binary"]["status"], "unavailable")

    def test_collection_reads_only_allowed_inputs_and_does_not_write(self):
        before = {path.relative_to(self.root): path.read_bytes() for path in self.root.rglob("*") if path.is_file()}
        allowed = {self.root / name for name in (
            "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "schemas/file-tile-config.schema.json",
            "target/.future-incompat-report.json",
        )}
        original = Path.open
        def open_file(path, mode="r", *args, **kwargs):
            self.assertIn(path, allowed)
            self.assertEqual(mode, "rb")
            return original(path, mode, *args, **kwargs)
        with patch.object(Path, "open", open_file), patch("dev.create_native_fixture", side_effect=AssertionError("fixture effect")):
            status, _ = self.collect()
        self.assertEqual(status, 0)
        after = {path.relative_to(self.root): path.read_bytes() for path in self.root.rglob("*") if path.is_file()}
        self.assertEqual(after, before)
        self.assertTrue(all(command[0] in {"rustc", "cargo", "git"} for command in self.commands))
        self.assertTrue(all(command[:2] == ["cargo", "metadata"] for command in self.commands if command[0] == "cargo"))


if __name__ == "__main__":
    unittest.main()
