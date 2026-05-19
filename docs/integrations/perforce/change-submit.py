#!/usr/bin/env python3
"""
Solarxy P4 trigger script — `change-submit` variant.

Runs solarxy-cli analyze against the model files in a pending changelist.
Exit code dictates whether Helix Core accepts the submit:

  0  → accept (no model files in changelist, or validation passed)
  1  → reject — validation found errors (artist must fix)
  2  → reject — tool error (admin attention required; see stderr)

Trigger registration (run `p4 triggers` once as an admin):

    Triggers:
        solarxy-validate change-submit //depot/Project/... \\
            "/usr/bin/python3 /opt/solarxy/change-submit.py %changelist%"

Files in `//depot/Project/` are scanned. The `solarxy.toml` is read from the
depot path defined in `CONFIG_DEPOT_PATH` below; override at runtime via
the SOLARXY_CONFIG_DEPOT_PATH environment variable.

Requires: python3, p4 (in PATH for the trigger user), solarxy-cli (in PATH).
Tested on Helix Core 2024.2 / Python 3.10+.
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

# Configure these for your studio. Override via env vars for ad-hoc testing.
CONFIG_DEPOT_PATH = os.environ.get(
    "SOLARXY_CONFIG_DEPOT_PATH", "//depot/Project/solarxy.toml"
)
ASSET_EXTENSIONS = {".glb", ".gltf", ".obj", ".stl", ".ply", ".fbx"}
SOLARXY_CLI = os.environ.get("SOLARXY_CLI", "solarxy-cli")

# Exit codes — semantically distinct so the trigger admin can route them.
EXIT_ACCEPT = 0
EXIT_VALIDATION_FAILED = 1
EXIT_TOOL_ERROR = 2


@dataclass(frozen=True)
class PendingFile:
    """A file pending in the changelist."""

    depot_path: str
    action: str   # "add", "edit", "delete", "branch", "integrate"


def main(changelist: str) -> int:
    try:
        pending = list_pending_files(changelist)
    except subprocess.CalledProcessError as e:
        print_tool_error(
            f"`p4 describe` failed for changelist {changelist}: {e}")
        return EXIT_TOOL_ERROR

    asset_files = [
        f for f in pending
        if Path(f.depot_path).suffix.lower() in ASSET_EXTENSIONS
        and f.action != "delete"
    ]
    if not asset_files:
        return EXIT_ACCEPT

    with tempfile.TemporaryDirectory(prefix="solarxy-p4-") as workdir:
        try:
            local_assets = print_pending_to_dir(
                asset_files, changelist, workdir)
            config_local = print_config_to_dir(workdir)
        except subprocess.CalledProcessError as e:
            print_tool_error(f"`p4 print` failed: {e}")
            return EXIT_TOOL_ERROR

        try:
            proc = subprocess.run(
                [
                    SOLARXY_CLI, "analyze",
                    "--adapter", "generic",
                    "--adapter-format", "json",
                    "--config", str(config_local),
                    "--paths", *[str(p) for p in local_assets],
                    "--fail-on", "error",
                ],
                capture_output=True, text=True,
            )
        except FileNotFoundError:
            print_tool_error(
                f"`{SOLARXY_CLI}` not found in PATH. Install with "
                "`brew install solarxy-cli` / Flatpak / portable .zip."
            )
            return EXIT_TOOL_ERROR

        if proc.returncode == 0:
            return EXIT_ACCEPT

        # Validation produced findings. Render a structured reject message.
        print_rejection(proc.stdout, asset_files)
        return EXIT_VALIDATION_FAILED


def list_pending_files(changelist: str) -> list[PendingFile]:
    """List depot paths in the pending changelist via `p4 -Ztag describe -s`."""
    result = subprocess.run(
        ["p4", "-Ztag", "describe", "-s", changelist],
        check=True, capture_output=True, text=True,
    )
    # -Ztag output uses `... depotFileN /depot/path` and `... actionN edit`
    # patterns. Pair them up by N.
    depot_files: dict[int, str] = {}
    actions: dict[int, str] = {}
    for line in result.stdout.splitlines():
        if m := re.match(r"^\.\.\. depotFile(\d+) (.+)$", line):
            depot_files[int(m.group(1))] = m.group(2)
        elif m := re.match(r"^\.\.\. action(\d+) (\w+)$", line):
            actions[int(m.group(1))] = m.group(2)
    return [
        PendingFile(depot_path=depot_files[i], action=actions.get(i, "edit"))
        for i in sorted(depot_files)
    ]


def print_pending_to_dir(
    files: list[PendingFile], changelist: str, workdir: str,
) -> list[Path]:
    """`p4 print -o` the pending file revisions into workdir, returning local paths."""
    workpath = Path(workdir)
    out: list[Path] = []
    for f in files:
        # `@=<cl>` references the pending shelved revision (or the current
        # workspace revision if not shelved). For change-submit triggers,
        # this is the revision the user is about to commit.
        depot_ref = f"{f.depot_path}@={changelist}"
        # Sanitize depot path → local filename. Preserve extension.
        safe = re.sub(r"[^a-zA-Z0-9_.-]+", "_", f.depot_path.lstrip("/"))
        local = workpath / safe
        subprocess.run(
            ["p4", "print", "-q", "-o", str(local), depot_ref],
            check=True, capture_output=True,
        )
        out.append(local)
    return out


def print_config_to_dir(workdir: str) -> Path:
    """`p4 print` the project's solarxy.toml to workdir."""
    local = Path(workdir) / "solarxy.toml"
    subprocess.run(
        ["p4", "print", "-q", "-o", str(local), CONFIG_DEPOT_PATH],
        check=True, capture_output=True,
    )
    return local


def print_rejection(json_blob: str, asset_files: list[PendingFile]) -> None:
    """
    Render a P4-trigger-style rejection message to stderr (which P4 surfaces
    to the artist's submit dialog).
    """
    import json as _json
    try:
        report = _json.loads(json_blob)
    except _json.JSONDecodeError:
        # Fall back to raw output rather than swallowing it.
        sys.stderr.write(
            "\nSubmit blocked by solarxy-validate (parse error):\n\n")
        sys.stderr.write(json_blob)
        sys.stderr.write("\n")
        return

    # Build a depot-path lookup so we can show the //depot/... form, not the
    # munged tempdir filename.
    depot_for_local: dict[str, str] = {}
    for f in asset_files:
        safe = re.sub(r"[^a-zA-Z0-9_.-]+", "_", f.depot_path.lstrip("/"))
        depot_for_local[safe] = f.depot_path

    sys.stderr.write("\nSubmit blocked by solarxy-validate:\n\n")
    for finding in report.get("findings", []):
        if finding.get("error_count", 0) == 0:
            continue
        local_name = Path(finding["path"]).name
        depot = depot_for_local.get(local_name, finding["path"])
        sys.stderr.write(f"  {depot} — {finding['error_count']} error(s)\n")
        # Surface up to 3 issues per file; truncate the rest.
        issues = [i for i in finding.get(
            "issues", []) if i.get("severity") == "error"]
        for issue in issues[:3]:
            scope_idx = issue.get("scope_index")
            scope_str = (
                f"{issue['scope']} {scope_idx}"
                if scope_idx is not None
                else issue["scope"]
            )
            sys.stderr.write(
                f"    {issue['kind']} ({scope_str}): {issue['message']}\n"
            )
        if len(issues) > 3:
            sys.stderr.write(f"    ... and {len(issues) - 3} more error(s)\n")
        # Per-file "open locally" hint.
        sys.stderr.write(f"  Open locally:\n    solarxy \"{depot}#head\"\n\n")

    sys.stderr.write(
        "To bypass for emergencies (admin password may be required):\n"
        "  p4 submit -f submitunchanged-fail\n"
    )


def print_tool_error(msg: str) -> None:
    sys.stderr.write(
        "\nSolarxy trigger failed to run (tool error, not your asset):\n"
    )
    sys.stderr.write(f"  {msg}\n")
    sys.stderr.write(
        "Contact your P4 admin. To allow the submit through despite the "
        "trigger error, the admin can temporarily disable the trigger via "
        "`p4 triggers` (this requires admin privileges).\n"
    )


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.stderr.write("usage: change-submit.py <changelist>\n")
        sys.exit(EXIT_TOOL_ERROR)
    sys.exit(main(sys.argv[1]))
