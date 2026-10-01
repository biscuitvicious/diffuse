#!/usr/bin/env python3
"""Prune older GitHub Actions artifacts to keep total artifact storage under a quota limit and max age."""

import concurrent.futures
from datetime import datetime, timezone, timedelta
import json
import os
import subprocess
import sys


def parse_iso8601(ts: str) -> datetime | None:
    if not ts:
        return None
    try:
        return datetime.fromisoformat(ts.replace("Z", "+00:00"))
    except ValueError:
        return None


def main() -> None:
    repo = os.environ.get("GITHUB_REPOSITORY")
    if not repo:
        print("Error: GITHUB_REPOSITORY environment variable not set.", file=sys.stderr)
        sys.exit(1)

    max_mb_str = os.environ.get("MAX_ARTIFACTS_MB", "100")
    try:
        max_mb = int(max_mb_str)
    except ValueError:
        max_mb = 100
    max_bytes = max_mb * 1024 * 1024

    max_days_str = os.environ.get("MAX_ARTIFACTS_DAYS", "10")
    try:
        max_days = int(max_days_str)
    except ValueError:
        max_days = 10
    cutoff_time = datetime.now(timezone.utc) - timedelta(days=max_days)

    print(
        f"Checking artifacts for {repo} "
        f"(storage limit: {max_mb} MB, max age: {max_days} days)..."
    )
    proc = subprocess.run(
        ["gh", "api", f"repos/{repo}/actions/artifacts?per_page=100", "--paginate"],
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        print(f"Warning: Could not fetch artifacts: {proc.stderr.strip()}", file=sys.stderr)
        # Do not fail the workflow if GitHub API is unreachable
        sys.exit(0)

    decoder = json.JSONDecoder()
    content = proc.stdout.strip()
    artifacts = []
    idx = 0
    while idx < len(content):
        content = content[idx:].lstrip()
        if not content:
            break
        try:
            obj, idx = decoder.raw_decode(content)
            artifacts.extend(obj.get("artifacts", []))
        except Exception as e:
            print(f"Error parsing JSON chunk: {e}", file=sys.stderr)
            break

    if not artifacts:
        print("No existing artifacts found.")
        return

    # Sort from newest to oldest by creation date, then ID
    artifacts.sort(
        key=lambda a: (a.get("created_at", ""), a.get("id", 0)),
        reverse=True,
    )

    total_bytes = sum(a.get("size_in_bytes", 0) for a in artifacts)
    total_mb = total_bytes / (1024 * 1024)
    print(f"Found {len(artifacts)} existing artifact(s) totaling {total_mb:.2f} MB.")

    running_total = 0
    keep = []
    delete = []

    for a in artifacts:
        size = a.get("size_in_bytes", 0)
        created_dt = parse_iso8601(a.get("created_at", ""))
        is_stale_by_age = created_dt is not None and created_dt < cutoff_time

        if not is_stale_by_age and (running_total + size <= max_bytes):
            running_total += size
            keep.append(a)
        else:
            reason = "age > 10d" if is_stale_by_age else "exceeds 100MB budget"
            delete.append((a, reason))

    keep_mb = running_total / (1024 * 1024)
    print(f"Retaining {len(keep)} newest artifact(s) ({keep_mb:.2f} MB within policy).")

    if not delete:
        print("All artifacts comply with storage budget and retention limits. Nothing to prune.")
        return

    delete_bytes = sum(a.get("size_in_bytes", 0) for a, _ in delete)
    delete_mb = delete_bytes / (1024 * 1024)
    print(f"Pruning {len(delete)} older artifact(s) totaling {delete_mb:.2f} MB...")

    def delete_artifact(entry: tuple[dict, str]) -> None:
        a, reason = entry
        aid = a["id"]
        name = a.get("name", "unknown")
        size_mb = a.get("size_in_bytes", 0) / (1024 * 1024)
        res = subprocess.run(
            ["gh", "api", "-X", "DELETE", f"repos/{repo}/actions/artifacts/{aid}"],
            capture_output=True,
            text=True,
        )
        if res.returncode == 0:
            print(f"  ✓ Deleted artifact {aid} ({name}, {size_mb:.2f} MB, {reason})")
        else:
            print(f"  ✗ Failed to delete artifact {aid}: {res.stderr.strip()}", file=sys.stderr)

    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as executor:
        list(executor.map(delete_artifact, delete))

    print(f"Pruning complete. Freed {delete_mb:.2f} MB.")


if __name__ == "__main__":
    main()
