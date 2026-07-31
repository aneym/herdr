#!/usr/bin/env python3
"""Prefetch Zig package dependencies when Zig's HTTP client cannot reach them.

Usage:
  ZIG_GLOBAL_CACHE_DIR=.zig-cache python3 scripts/prefetch_zig_dependencies.py
  python3 scripts/prefetch_zig_dependencies.py --zig /path/to/zig --cache-dir .zig-cache

The script scans every build.zig.zon under vendor/libghostty-vt and recursively
scans fetched packages. Archives are downloaded with curl, Git dependencies are
checked out at their pinned revision, and Zig verifies every package against the
hash declared in its manifest before accepting it into the global cache.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path
from urllib.parse import urlsplit

DEPENDENCY_PATTERN = re.compile(
    r'\.url\s*=\s*"([^"]+)"\s*,\s*\.hash\s*=\s*"([^"]+)"',
    re.DOTALL,
)
ARCHIVE_SUFFIXES = (".tar.gz", ".tar.zst", ".tar.xz", ".tgz", ".zip")


@dataclass(frozen=True)
class Dependency:
    url: str
    expected_hash: str


def dependencies_from_manifest(path: Path) -> list[Dependency]:
    return [
        Dependency(url, expected_hash)
        for url, expected_hash in DEPENDENCY_PATTERN.findall(path.read_text())
    ]


def manifest_paths(source_root: Path, cache_dir: Path) -> list[Path]:
    paths = list(source_root.rglob("build.zig.zon"))
    package_cache = cache_dir / "p"
    if package_cache.is_dir():
        paths.extend(package_cache.rglob("build.zig.zon"))
    return sorted(set(paths))


def cached(cache_dir: Path, expected_hash: str) -> bool:
    return (cache_dir / "p" / expected_hash).is_dir()


def run(command: list[str], *, cwd: Path | None = None, capture: bool = False) -> str:
    result = subprocess.run(
        command,
        cwd=cwd,
        check=True,
        text=True,
        capture_output=capture,
    )
    return result.stdout.strip() if capture else ""


def archive_suffix(url: str) -> str:
    path = urlsplit(url).path
    return next((suffix for suffix in ARCHIVE_SUFFIXES if path.endswith(suffix)), ".archive")


def fetch_archive(dependency: Dependency, zig: str, cache_dir: Path, temp_dir: Path) -> str:
    archive = temp_dir / f"package{archive_suffix(dependency.url)}"
    run(["curl", "-fL", "--retry", "3", "--retry-delay", "1", dependency.url, "-o", str(archive)])
    return run(
        [zig, "fetch", str(archive), "--global-cache-dir", str(cache_dir)],
        capture=True,
    )


def fetch_git(dependency: Dependency, zig: str, cache_dir: Path, temp_dir: Path) -> str:
    url_and_revision = dependency.url.removeprefix("git+")
    repository_url, separator, revision = url_and_revision.rpartition("#")
    if not separator or not repository_url or not revision:
        raise ValueError(f"Git dependency must include a pinned revision: {dependency.url}")
    checkout = temp_dir / "checkout"
    run(["git", "clone", "--filter=blob:none", "--no-checkout", repository_url, str(checkout)])
    run(["git", "checkout", "--detach", revision], cwd=checkout)
    return run(
        [zig, "fetch", str(checkout), "--global-cache-dir", str(cache_dir)],
        capture=True,
    )


def fetch(dependency: Dependency, zig: str, cache_dir: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="herdr-zig-fetch-") as directory:
        temp_dir = Path(directory)
        if dependency.url.startswith("git+"):
            actual_hash = fetch_git(dependency, zig, cache_dir, temp_dir)
        else:
            actual_hash = fetch_archive(dependency, zig, cache_dir, temp_dir)
    if actual_hash != dependency.expected_hash:
        raise RuntimeError(
            f"hash mismatch for {dependency.url}:\n"
            f"expected: {dependency.expected_hash}\n"
            f"actual:   {actual_hash}"
        )


def prefetch(source_root: Path, cache_dir: Path, zig: str) -> tuple[int, int]:
    cache_dir.mkdir(parents=True, exist_ok=True)
    known: dict[str, str] = {}
    fetched_count = 0

    while True:
        for manifest in manifest_paths(source_root, cache_dir):
            for dependency in dependencies_from_manifest(manifest):
                previous = known.setdefault(dependency.url, dependency.expected_hash)
                if previous != dependency.expected_hash:
                    raise RuntimeError(
                        f"conflicting hashes for {dependency.url}: {previous} and {dependency.expected_hash}"
                    )

        pending = [
            Dependency(url, expected_hash)
            for url, expected_hash in sorted(known.items())
            if not cached(cache_dir, expected_hash)
        ]
        if not pending:
            return len(known), fetched_count

        for dependency in pending:
            print(f"fetching {dependency.url}", flush=True)
            fetch(dependency, zig, cache_dir)
            fetched_count += 1


def parse_args() -> argparse.Namespace:
    project_root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--source-root",
        type=Path,
        default=project_root / "vendor" / "libghostty-vt",
        help="root containing Zig package manifests",
    )
    parser.add_argument(
        "--cache-dir",
        type=Path,
        default=Path(os.environ.get("ZIG_GLOBAL_CACHE_DIR", project_root / ".zig-cache")),
        help="Zig global cache directory",
    )
    parser.add_argument("--zig", default=os.environ.get("ZIG", "zig"), help="Zig executable")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    dependency_count, fetched_count = prefetch(
        args.source_root.resolve(),
        args.cache_dir.resolve(),
        args.zig,
    )
    print(
        f"verified {dependency_count} dependencies in {args.cache_dir.resolve()} "
        f"({fetched_count} fetched)"
    )


if __name__ == "__main__":
    main()
