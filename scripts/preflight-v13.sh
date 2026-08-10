#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

need() { command -v "$1" >/dev/null 2>&1 || { echo "missing required tool: $1" >&2; exit 127; }; }
need cargo
need node
need npm

printf '\n[1/6] Rust workspace check\n'
cargo check --workspace --all-targets

printf '\n[2/6] Rust workspace tests\n'
cargo test --workspace

printf '\n[3/6] VS Code install/compile\n'
pushd integrations/vscode >/dev/null
npm ci
npm run compile

printf '\n[4/6] VSIX package\n'
VERSION="$(node -p "require('./package.json').version")"
OUT="ckb-vscode-${VERSION}.vsix"
npx --yes @vscode/vsce@latest package --out "$OUT"
sha256sum "$OUT" > "${OUT}.sha256"
unzip -t "$OUT" >/dev/null
popd >/dev/null

printf '\n[5/6] Protocol/schema sanity\n'
if compgen -G 'schemas/*.json' >/dev/null; then
  for f in schemas/*.json; do node -e 'JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"))' "$f"; done
fi

printf '\n[6/6] Optional JetBrains build\n'
if [[ -x integrations/jetbrains/gradlew ]]; then
  (cd integrations/jetbrains && ./gradlew build)
else
  echo 'JetBrains wrapper not present/executable; skipped.'
fi

printf '\nPRE-VPS PREFLIGHT PASSED\n'
printf 'VSIX: integrations/vscode/%s\n' "$OUT"
printf 'SHA256: integrations/vscode/%s.sha256\n' "$OUT"
