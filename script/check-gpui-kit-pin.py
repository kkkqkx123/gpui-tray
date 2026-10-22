#!/usr/bin/env python3
"""Verify that this crate pins the same GPUI snapshot the published GPUI Kit
release pins.

The tray takes the application's `gpui::App` and dispatches the application's
`gpui::Action`. When the two crates resolve different snapshots, the
application's lock carries two `gpui-pre` versions and the two `App` types do
not connect: the call site stops compiling, or worse, an action from one
snapshot is handed to the other.

The check reads the latest published GPUI Kit manifest from crates.io and
compares its snapshot requirements with the ones in this manifest.
"""
from __future__ import annotations

import json
import re
import sys
import tarfile
import tomllib
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
INDEX_URL = "https://index.crates.io/gp/ui/gpui-kit"
CRATE_URL = "https://static.crates.io/crates/gpui-kit"


def fetch(url: str) -> bytes:
    with urllib.request.urlopen(url, timeout=60) as response:
        return response.read()


def latest_published_version() -> str:
    """The newest non-yanked GPUI Kit version on crates.io."""
    versions = []
    for line in fetch(INDEX_URL).decode().splitlines():
        if not line.strip():
            continue
        entry = json.loads(line)
        if not entry.get("yanked"):
            versions.append(entry["vers"])

    def sort_key(version: str) -> tuple[int, ...]:
        return tuple(int(part) for part in version.split("."))

    if not versions:
        raise SystemExit("no published GPUI Kit version found on crates.io")
    return max(versions, key=sort_key)


def published_manifest(version: str) -> dict:
    """The manifest of a published GPUI Kit crate."""
    archive = ROOT / "target" / f"gpui-kit-{version}.crate"
    archive.parent.mkdir(parents=True, exist_ok=True)
    archive.write_bytes(fetch(f"{CRATE_URL}/gpui-kit-{version}.crate"))
    with tarfile.open(archive) as tar:
        member = tar.getmember(f"gpui-kit-{version}/Cargo.toml")
        extracted = tar.extractfile(member)
        assert extracted is not None
        return tomllib.loads(extracted.read().decode())


def requirement(manifest: dict, dependency: str, package: str | None = None) -> str | None:
    """The version requirement of one dependency of a manifest."""
    entry = manifest.get("dependencies", {}).get(dependency)
    if entry is None:
        return None
    if isinstance(entry, str):
        return entry
    if package is not None and entry.get("package") != package:
        return None
    return entry.get("version")


def pinned_version(text: str) -> str:
    """The version a requirement names, without its operator."""
    match = re.search(r"(\d+(?:\.\d+)*)", text)
    if match is None:
        raise SystemExit(f"cannot read a version out of {text!r}")
    return match.group(1)


def main() -> int:
    own = tomllib.loads((ROOT / "Cargo.toml").read_text())

    version = latest_published_version()
    kit = published_manifest(version)
    print(f"published GPUI Kit: {version}")

    checks = [
        (
            "the gpui-pre snapshot",
            requirement(own, "gpui-kit-backend", "gpui-pre"),
            requirement(kit, "gpui", "gpui-pre"),
        ),
        (
            "the gpui-fast snapshot",
            requirement(own, "gpui-fast-backend", "gpui-fast"),
            requirement(kit, "gpui-fast", "gpui-fast"),
        ),
    ]

    problems = []
    for label, ours, theirs in checks:
        if ours is None or theirs is None:
            problems.append(f"{label}: a requirement is missing (ours={ours}, gpui-kit={theirs})")
            continue
        our_version = pinned_version(ours)
        their_version = pinned_version(theirs)
        if our_version != their_version:
            problems.append(
                f"{label}: this crate pins {our_version} but GPUI Kit {version} pins "
                f"{their_version}; the application would link two GPUI snapshots"
            )
        else:
            print(f"{label}: {our_version} (matches GPUI Kit {version})")

    if problems:
        for problem in problems:
            print(f"::error::{problem}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
