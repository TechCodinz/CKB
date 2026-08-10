"""CKB Live Reality stack detection for Python projects.

Reads dependency metadata only. It never imports the target application,
executes user modules, reads environment secrets, or inspects traffic/payloads.
"""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Dict, Iterable, List, Set

GROUPS: Dict[str, Dict[str, Set[str]]] = {
    "frameworks": {
        "fastapi": {"fastapi"},
        "starlette": {"starlette"},
        "django": {"django"},
        "flask": {"flask"},
        "quart": {"quart"},
        "litestar": {"litestar", "starlite"},
    },
    "databases": {
        "sqlalchemy": {"sqlalchemy"},
        "postgres": {"psycopg", "psycopg2", "asyncpg"},
        "mysql": {"mysqlclient", "pymysql", "aiomysql"},
        "sqlite": {"aiosqlite"},
        "mongodb": {"pymongo", "motor", "mongoengine"},
        "django-orm": {"django"},
    },
    "caches": {
        "redis": {"redis", "aioredis"},
        "memcached": {"pymemcache", "python-memcached"},
    },
    "messaging": {
        "celery": {"celery"},
        "rq": {"rq"},
        "kafka": {"confluent-kafka", "kafka-python", "aiokafka"},
        "rabbitmq": {"pika", "aio-pika"},
        "sqs": {"boto3", "aiobotocore"},
    },
    "websockets": {
        "websockets": {"websockets"},
        "socketio": {"python-socketio"},
        "channels": {"channels"},
    },
    "http_clients": {
        "httpx": {"httpx"},
        "aiohttp": {"aiohttp"},
        "requests": {"requests"},
        "urllib3": {"urllib3"},
    },
}

_NAME = re.compile(r"^[A-Za-z0-9_.-]+")


def _normalise(name: str) -> str:
    return re.sub(r"[-_.]+", "-", name.strip().lower())


def _requirement_name(line: str) -> str:
    value = line.split("#", 1)[0].strip()
    if not value or value.startswith(("-", "http://", "https://", "git+")):
        return ""
    match = _NAME.match(value)
    return _normalise(match.group(0)) if match else ""


def _read_requirements(root: Path) -> Set[str]:
    names: Set[str] = set()
    for candidate in sorted(root.glob("requirements*.txt")):
        try:
            for line in candidate.read_text(encoding="utf-8", errors="ignore").splitlines():
                name = _requirement_name(line)
                if name:
                    names.add(name)
        except OSError:
            continue
    return names


def _read_pyproject(root: Path) -> Set[str]:
    file = root / "pyproject.toml"
    if not file.exists():
        return set()
    try:
        text = file.read_text(encoding="utf-8", errors="ignore")
    except OSError:
        return set()

    names: Set[str] = set()
    # This is intentionally a bounded metadata scanner rather than a TOML
    # evaluator. It recognizes quoted dependency names from common dependency
    # blocks and Poetry/PDM-style tables without executing any project code.
    for quoted in re.findall(r"[\"']([A-Za-z0-9_.-]+)(?:\[[^\]]+\])?(?:\s*[<>=!~^].*?)?[\"']", text):
        names.add(_normalise(quoted))
    for key in re.findall(r"(?m)^\s*([A-Za-z0-9_.-]+)\s*=\s*(?:[\"'{\[])" , text):
        names.add(_normalise(key))
    return names


def _read_pipfile(root: Path) -> Set[str]:
    file = root / "Pipfile"
    if not file.exists():
        return set()
    try:
        text = file.read_text(encoding="utf-8", errors="ignore")
    except OSError:
        return set()
    return {_normalise(name) for name in re.findall(r"(?m)^\s*([A-Za-z0-9_.-]+)\s*=", text)}


def _read_package_metadata(root: Path) -> Set[str]:
    names = set()
    names.update(_read_requirements(root))
    names.update(_read_pyproject(root))
    names.update(_read_pipfile(root))
    return names


def _matches(packages: Set[str], aliases: Iterable[str]) -> bool:
    normalised = {_normalise(value) for value in aliases}
    return bool(packages & normalised)


def detect_runtime_stack(root_dir: str | Path = ".") -> Dict[str, object]:
    root = Path(root_dir).expanduser().resolve()
    packages = _read_package_metadata(root)
    detected: Dict[str, List[str]] = {}
    for category, entries in GROUPS.items():
        detected[category] = [name for name, aliases in entries.items() if _matches(packages, aliases)]

    suggestions: List[str] = []
    frameworks = detected["frameworks"]
    if any(name in frameworks for name in ("fastapi", "starlette", "litestar", "quart")):
        suggestions.append("Wrap the ASGI application with CkbASGIMiddleware(app, live).")
    if "django" in frameworks:
        suggestions.append("Use CkbASGIMiddleware for Django ASGI or CkbWSGIMiddleware for Django WSGI.")
    if "flask" in frameworks:
        suggestions.append("Wrap Flask's WSGI application with CkbWSGIMiddleware.")
    if detected["databases"]:
        suggestions.append("Wrap DB-API connections with instrument_db(...); SQL text and parameters remain outside telemetry.")
    if detected["caches"]:
        suggestions.append("Instrument cache operation boundaries as metadata-only spans; never copy keys or values.")
    if detected["messaging"]:
        suggestions.append("Instrument producer/consumer boundaries so queue/event transitions can be retraced across observed spans.")
    if detected["websockets"]:
        suggestions.append("Instrument WebSocket send/handler boundaries without recording message bodies.")
    suggestions.append("Preserve code.file.path + code.function.name on custom spans for exact static/runtime fusion.")

    return {
        "root_dir": str(root),
        "source": "python-dependency-metadata-only",
        "packages_seen": len(packages),
        **detected,
        "suggestions": suggestions,
    }


def main() -> None:
    print(json.dumps(detect_runtime_stack(), indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
