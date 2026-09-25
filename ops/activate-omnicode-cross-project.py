"""Activate the CKB/OmniCode/ADA cross-project production QA loop."""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time
from urllib.error import HTTPError
from urllib.request import Request, urlopen

BASE = Path("/opt/app-platform")
ADA_REPO = BASE / "src/ada-scanner"
ADA_BRANCH = "fix/restore-omnicode-ada-handoff"
SERVICE = "omnicode-validation-worker.service"


def run(args, *, check=True, capture=True, cwd=None):
    result = subprocess.run(
        args,
        cwd=cwd,
        capture_output=capture,
        text=True,
    )
    if check and result.returncode:
        detail = (result.stderr or result.stdout or "")[-1500:] if capture else ""
        raise RuntimeError(f"{args[0]} failed ({result.returncode}): {detail}")
    return result


def find_worker_target() -> Path:
    unit = run(["systemctl", "cat", SERVICE], check=False).stdout or ""
    match = re.search(r"(?m)^\s*ExecStart\s*=.*?(/[^\s;]+omnicode-validation-worker\.mjs)", unit)
    if match:
        path = Path(match.group(1))
        if path.exists():
            return path

    for candidate in (
        BASE / "src/ckb-runtime/ops/omnicode-validation-worker.mjs",
        BASE / "src/CKB/ops/omnicode-validation-worker.mjs",
        BASE / "src/ckb-cloud/ops/omnicode-validation-worker.mjs",
    ):
        if candidate.exists():
            return candidate
    raise RuntimeError("Could not locate the running OmniCode validation worker source.")


def repo_root_for(target: Path) -> Path:
    current = target.parent
    while current != current.parent:
        if (current / ".git").exists():
            return current
        current = current.parent
    raise RuntimeError(f"Could not identify Git repository for {target}")


def http_status(url: str, *, method="GET", body=None, timeout=20):
    data = json.dumps(body).encode() if body is not None else None
    request = Request(
        url,
        data=data,
        headers={"Content-Type": "application/json"} if data is not None else {},
        method=method,
    )
    try:
        response = urlopen(request, timeout=timeout)
    except HTTPError as error:
        response = error
    with response as handle:
        payload = handle.read().decode("utf-8", "replace")
        return handle.status, payload[:1200]


def ada_handoff_active():
    health_status, _ = http_status("https://api.adascannerpro.com/health")
    route_status, _ = http_status(
        "https://api.adascannerpro.com/integrations/omnicode/scans",
        method="POST",
        body={
            "buildId": "deployment-route-probe",
            "projectId": "deployment-route-probe",
            "url": "https://example.com",
        },
    )
    return health_status == 200 and route_status == 401


def activate_ada():
    if not ADA_REPO.is_dir():
        raise RuntimeError(f"ADA source repository missing: {ADA_REPO}")

    print("===== ADA OMNICODE HANDOFF =====", flush=True)
    run([
        "git", "-C", str(ADA_REPO), "fetch", "origin",
        f"{ADA_BRANCH}:refs/remotes/origin/{ADA_BRANCH}",
    ])
    content = run([
        "git", "-C", str(ADA_REPO), "show",
        f"origin/{ADA_BRANCH}:deploy/vps/activate_omnicode_handoff.py",
    ]).stdout

    with tempfile.NamedTemporaryFile("w", suffix=".py", delete=False) as handle:
        handle.write(content)
        script_path = Path(handle.name)
    try:
        compiled = run(["python3", "-m", "py_compile", str(script_path)], check=False)
        if compiled.returncode:
            raise RuntimeError("ADA activation script failed Python compilation.")
        result = subprocess.run(["python3", str(script_path)])
        if result.returncode:
            raise RuntimeError("ADA OmniCode handoff activation failed.")
    finally:
        script_path.unlink(missing_ok=True)


def activate_worker():
    print("===== CKB / OMNICODE VALIDATION WORKER =====", flush=True)
    target = find_worker_target()
    repo = repo_root_for(target)
    run(["git", "-C", str(repo), "fetch", "origin", "main"])
    source = run([
        "git", "-C", str(repo), "show",
        "origin/main:ops/omnicode-validation-worker.mjs",
    ]).stdout

    backup_dir = BASE / "backups" / time.strftime("ckb-omnicode-worker-%Y%m%dT%H%M%SZ", time.gmtime())
    backup_dir.mkdir(parents=True, mode=0o700)
    backup = backup_dir / target.name
    shutil.copy2(target, backup)

    # Preserve the .mjs extension during syntax checking. A temporary filename
    # ending in ".mjs.new" is treated as an unknown/CommonJS-like input by some
    # Node versions and can reject valid ESM imports.
    temp = target.with_name(target.stem + ".new" + target.suffix)
    temp.write_text(source)
    os.chmod(temp, target.stat().st_mode & 0o777)

    syntax = run(["node", "--check", str(temp)], check=False)
    if syntax.returncode:
        detail = (syntax.stderr or syntax.stdout or "").strip()
        temp.unlink(missing_ok=True)
        raise RuntimeError(
            "Updated validation worker failed node --check: " + detail[-1200:]
        )

    temp.replace(target)

    try:
        run(["systemctl", "restart", SERVICE])
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            active = run(["systemctl", "is-active", SERVICE], check=False).stdout.strip()
            if active == "active":
                break
            time.sleep(1)
        else:
            raise RuntimeError("Validation worker did not become active.")
    except Exception:
        shutil.copy2(backup, target)
        run(["systemctl", "restart", SERVICE], check=False)
        raise

    print("Validation worker active. Backup:", backup, flush=True)


def verify_public():
    print("===== PUBLIC CROSS-PROJECT VERIFICATION =====", flush=True)
    checks = [
        ("CKB API", "https://ckb-api.169.58.175.192.nip.io/health", 200),
        ("CKB Reality gateway", "https://ckb-mcp.169.58.175.192.nip.io/ready", 200),
        ("CKB model registry", "https://ckb-models.169.58.175.192.nip.io/health", 200),
        ("OmniCode health", "https://omnicode-pro.vercel.app/api/health", 200),
        ("OmniCode ready", "https://omnicode-pro.vercel.app/api/ready", 200),
        ("ADA health", "https://api.adascannerpro.com/health", 200),
        ("OmniCode release agent", "https://omni-release.169.58.175.192.nip.io/health", 200),
        ("Generated OmniCode app", "https://omni-jv044fqnx3qa.169.58.175.192.nip.io", 200),
    ]

    for label, url, expected in checks:
        status, body = http_status(url)
        print(f"{label}: HTTP {status}", flush=True)
        if status != expected:
            raise RuntimeError(f"{label} expected HTTP {expected}, got {status}")

    ada_status, _ = http_status(
        "https://api.adascannerpro.com/integrations/omnicode/scans",
        method="POST",
        body={
            "buildId": "deployment-route-probe",
            "projectId": "deployment-route-probe",
            "url": "https://example.com",
        },
    )
    print(f"ADA OmniCode trust probe: HTTP {ada_status}", flush=True)
    if ada_status != 401:
        raise RuntimeError(f"ADA OmniCode route expected fail-closed 401, got {ada_status}")

    for label, url in (
        ("OmniCode external-QA trust", "https://omnicode-pro.vercel.app/api/system/external-qa?limit=1"),
        ("OmniCode release-reconciler trust", "https://omnicode-pro.vercel.app/api/system/autonomous-release?limit=1"),
    ):
        status, _ = http_status(url)
        print(f"{label}: HTTP {status}", flush=True)
        if status != 401:
            raise RuntimeError(f"{label} expected configured/fail-closed HTTP 401, got {status}")


def show_worker_evidence():
    print("===== WORKER EVIDENCE =====", flush=True)
    # Give the worker a brief chance to establish its validation heartbeat.
    time.sleep(5)
    logs = run(
        ["journalctl", "-u", SERVICE, "--since", "2 minutes ago", "--no-pager", "-n", "80"],
        check=False,
    ).stdout
    safe_lines = [
        line for line in logs.splitlines()
        if (
            "[validation-worker] started" in line
            or "reconcile scheduled by OmniCode" in line
            or "completed" in line
        )
    ]
    for line in safe_lines[-20:]:
        print(line, flush=True)


def main():
    os.umask(0o077)
    if ada_handoff_active():
        print("===== ADA OMNICODE HANDOFF =====", flush=True)
        print("ADA handoff already active and fail-closed; skipping overlay.", flush=True)
    else:
        activate_ada()
    activate_worker()
    verify_public()
    show_worker_evidence()
    print("CROSS_PROJECT_QA_ACTIVATED", flush=True)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print("STOPPED:", str(error) if isinstance(error, RuntimeError) else type(error).__name__, flush=True)
        raise SystemExit(1)
