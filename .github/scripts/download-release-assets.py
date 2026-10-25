#!/usr/bin/env python3
"""Download official release assets with bounded parallelism and digest-bound reuse.

Authentication and proxy configuration stay in gh's existing process environment.
Completed cache entries never replace final package/signature verification.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time

LOG_LOCK = threading.Lock()


def log(message):
    with LOG_LOCK:
        print(message, flush=True)


def matches(path, size, digest):
    if path.is_symlink() or not path.is_file() or path.stat().st_size != size:
        return False
    actual = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            actual.update(chunk)
    return actual.hexdigest() == digest


def identity(repo, tag, asset):
    asset_id = int(asset["apiUrl"].rstrip("/").rsplit("/", 1)[-1])
    digest = asset["digest"]
    if not isinstance(digest, str) or not digest.startswith("sha256:"):
        raise ValueError("GitHub asset SHA-256 is required")
    digest = digest.removeprefix("sha256:")
    if len(digest) != 64 or any(char not in "0123456789abcdef" for char in digest):
        raise ValueError("Invalid GitHub asset digest")
    size = int(asset["size"])
    if asset_id <= 0 or size < 0:
        raise ValueError("Invalid GitHub asset identity or size")
    key = hashlib.sha256(json.dumps([repo, tag, asset_id, size, digest]).encode()).hexdigest()
    return asset_id, size, digest, key


def download(repo, tag, asset, output, cache, idle_timeout, total_timeout, cancelled):
    name = asset["name"]
    if cancelled.is_set():
        raise RuntimeError(f"Asset download cancelled: {name}")
    if name != Path(name).name or name in {".", ".."}:
        raise ValueError("Invalid asset name")
    asset_id, size, digest, key = identity(repo, tag, asset)
    destination = output / name
    cached = cache / key
    if matches(cached, size, digest):
        shutil.copyfile(cached, destination)
        log(f"cache-hit: {name} ({size} bytes)")
        return
    partial = output / f"{name}.part"
    started = progressed = reported = time.monotonic()
    received = 0
    log(f"download-start: {name} ({size} bytes)")
    with partial.open("wb") as stream:
        process = subprocess.Popen(
            ["gh", "api", f"repos/{repo}/releases/assets/{asset_id}", "-H", "Accept: application/octet-stream"],
            stdout=stream,
            # Network errors can contain temporary signed redirect URLs.
            stderr=subprocess.DEVNULL,
        )
        try:
            while process.poll() is None:
                time.sleep(1)
                if cancelled.is_set():
                    raise RuntimeError(f"Asset download cancelled: {name}")
                now = time.monotonic()
                current = partial.stat().st_size
                if current != received:
                    progressed, received = now, current
                if current > size:
                    raise RuntimeError(f"Asset exceeded expected size: {name}")
                if now - progressed > idle_timeout or now - started > total_timeout:
                    raise TimeoutError(f"Asset download exceeded its progress/time boundary: {name}")
                if now - reported >= 30:
                    log(f"download-progress: {name} {received}/{size} bytes")
                    reported = now
            if process.returncode:
                raise RuntimeError(f"Asset download failed: {name} (exit {process.returncode})")
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
    if not matches(partial, size, digest):
        raise RuntimeError(f"Asset size or SHA-256 mismatch: {name}")
    partial.replace(destination)
    with tempfile.NamedTemporaryFile(dir=cache, prefix=".asset-", delete=False) as stream:
        staging = Path(stream.name)
        try:
            with destination.open("rb") as source:
                shutil.copyfileobj(source, stream)
            stream.flush()
            staging.replace(cached)
        finally:
            staging.unlink(missing_ok=True)
    log(f"download-complete: {name} ({time.monotonic() - started:.1f}s)")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--release-json", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--idle-timeout", type=int, default=45)
    parser.add_argument("--asset-timeout", type=int, default=1200)
    parser.add_argument("names", nargs="+")
    args = parser.parse_args()
    if min(args.idle_timeout, args.asset_timeout) <= 0:
        parser.error("Timeouts must be positive")
    if len(set(args.names)) != len(args.names):
        parser.error("Asset names must be unique")
    release = json.loads(args.release_json.read_text())
    if release["tagName"] != args.tag or release["isDraft"] or release["isPrerelease"]:
        parser.error("Expected the requested published stable release")
    assets = []
    for name in args.names:
        candidates = [asset for asset in release["assets"] if asset["name"] == name]
        if len(candidates) != 1:
            parser.error(f"Expected one asset named {name}")
        assets.append(candidates[0])
    args.output.mkdir(parents=True, exist_ok=True)
    args.cache.mkdir(parents=True, exist_ok=True, mode=0o700)
    cancelled = threading.Event()
    with ThreadPoolExecutor(max_workers=4) as pool:
        futures = [pool.submit(download, args.repo, args.tag, asset, args.output, args.cache,
                               args.idle_timeout, args.asset_timeout, cancelled) for asset in assets]
        try:
            for future in as_completed(futures):
                future.result()
        except BaseException:
            cancelled.set()
            for future in futures:
                future.cancel()
            raise


if __name__ == "__main__":
    main()
