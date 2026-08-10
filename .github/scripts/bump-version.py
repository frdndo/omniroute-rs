#!/usr/bin/env python3
"""Auto version bump + tag helper (dipecah dari workflow supaya bisa di-test).

Rules:
- Version source of truth: src-tauri/tauri.conf.json
- Bump: minor jika ada commit `feat`/`feat(...)` sejak tag terakhir,
  selain itu patch. (0.x — major tidak dipakai.)
- Idempotent: NOOP jika versi sudah di-tag, atau commit terakhir sudah
  `chore(release):`.

Files yang di-update:
- rust-core/Cargo.toml ([workspace.package] version)
- rust-core/omniroute-{core,db,providers}/Cargo.toml
- src-tauri/Cargo.toml
- src-tauri/tauri.conf.json
- dashboard/package.json

Usage: python3 bump-version.py [--dry-run]
Output: "NOOP <alasan>" atau versi baru (satu baris terakhir).
"""
import json
import re
import subprocess
import sys
import os

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

VERSION_FILES = [
    "rust-core/Cargo.toml",            # [workspace.package] version
    "rust-core/omniroute-core/Cargo.toml",
    "rust-core/omniroute-db/Cargo.toml",
    "rust-core/omniroute-providers/Cargo.toml",
    "src-tauri/Cargo.toml",
    "src-tauri/tauri.conf.json",
    "dashboard/package.json",
]


def sh(*args: str) -> str:
    r = subprocess.run(args, capture_output=True, text=True, cwd=ROOT)
    return r.stdout.strip()


def current_version() -> str:
    with open(os.path.join(ROOT, "src-tauri/tauri.conf.json")) as f:
        return json.load(f)["version"]


def bump(version: str, minor: bool) -> str:
    major, mid, patch = (int(x) for x in version.split("."))
    if minor:
        return f"{major}.{mid + 1}.0"
    return f"{major}.{mid}.{patch + 1}"


def update_files(new: str) -> None:
    for rel in VERSION_FILES:
        path = os.path.join(ROOT, rel)
        with open(path) as f:
            src = f.read()
        if rel == "rust-core/Cargo.toml":
            src = re.sub(r'(^\[workspace\.package\]\nversion = ")[^"]+(")', rf"\g<1>{new}\g<2>", src, flags=re.M)
        elif rel.endswith(".json"):
            data = json.loads(src)
            data["version"] = new
            with open(path, "w") as f:
                json.dump(data, f, indent=2)
                f.write("\n")
            continue
        else:
            src = re.sub(r'(^version = ")[^"]+(")', rf"\g<1>{new}\g<2>", src, count=1, flags=re.M)
        with open(path, "w") as f:
            f.write(src)


def main() -> int:
    dry = "--dry-run" in sys.argv
    cur = current_version()

    # Guard 1: versi sudah di-tag → NOOP (anti infinite loop)
    tags = sh("git", "tag", "--list", "v*").split("\n")
    if f"v{cur}" in tags:
        print(f"NOOP versi {cur} sudah di-tag")
        return 0

    # Guard 2: commit terakhir sudah release commit → NOOP
    latest = sh("git", "log", "-1", "--pretty=%s")
    if latest.startswith("chore(release):"):
        print(f"NOOP commit terakhir sudah release ({latest})")
        return 0

    # Komit sejak tag terakhir (atau seluruh riwayat kalau belum ada tag)
    if f"v{cur}" in tags:
        commits = sh("git", "log", "--pretty=%s", f"v{cur}..HEAD").split("\n")
    else:
        commits = sh("git", "log", "--pretty=%s").split("\n")
    commits = [c for c in commits if c]

    minor = any(c.startswith("feat") or c.startswith("feat(") or c.startswith("feat!") for c in commits)
    new = bump(cur, minor)

    if dry:
        print(f"DRY-RUN: {cur} → {new} (minor={minor}, commits={len(commits)})")
        return 0

    update_files(new)
    print(new)
    return 0


if __name__ == "__main__":
    sys.exit(main())
