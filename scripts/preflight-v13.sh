#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

need() { command -v "$1" >/dev/null 2>&1 || { echo "missing required tool: $1" >&2; exit 127; }; }
need cargo
need node
need npm
need git
need sha256sum
need unzip

printf '\n[1/6] Rust workspace check\n'
cargo check --workspace --all-targets

printf '\n[2/6] Rust workspace tests\n'
cargo test --workspace --all-targets

printf '\n[3/6] VS Code install/compile\n'
pushd integrations/vscode >/dev/null
npm ci
npm run compile

printf '\n[4/6] VSIX package + provenance manifest\n'
VERSION="$(node -p "require('./package.json').version")"
OUT="ckb-vscode-${VERSION}.vsix"
npx --yes @vscode/vsce@latest package --out "$OUT"
sha256sum "$OUT" > "${OUT}.sha256"
unzip -t "$OUT" >/dev/null
SHA256="$(cut -d' ' -f1 "${OUT}.sha256")"
SOURCE_COMMIT="$(git -C "$ROOT" rev-parse HEAD)"
SOURCE_DIRTY=false
if [[ -n "$(git -C "$ROOT" status --porcelain)" ]]; then SOURCE_DIRTY=true; fi
node - "$VERSION" "$OUT" "$SHA256" "$SOURCE_COMMIT" "$SOURCE_DIRTY" > "${OUT}.release.json" <<'NODE'
const [version, artifact, sha256, sourceCommit, sourceDirty] = process.argv.slice(2);
process.stdout.write(JSON.stringify({
  schema: 'ckb-vscode-release-manifest-v1',
  publisher: 'TechCodinz',
  extension: 'ckb-vscode',
  version,
  artifact,
  sha256,
  sourceCommit,
  sourceDirty: sourceDirty === 'true',
  generatedAt: new Date().toISOString(),
}, null, 2) + '\n');
NODE
if [[ "$SOURCE_DIRTY" == "true" && "${CKB_ALLOW_DIRTY_RELEASE:-0}" != "1" ]]; then
  echo 'Refusing release artifact from a dirty worktree. Commit/stash changes and rerun.' >&2
  exit 4
fi
popd >/dev/null

printf '\n[5/6] Protocol/schema sanity\n'
if compgen -G 'schemas/*.json' >/dev/null; then
  for f in schemas/*.json; do node -e 'JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"))' "$f"; done
fi

printf '\n[6/6] Optional JetBrains build\n'
if [[ -x integrations/jetbrains/gradlew ]]; then
  (cd integrations/jetbrains && ./gradlew buildPlugin --stacktrace)
else
  echo 'JetBrains wrapper not present/executable; skipped.'
fi

printf '\nPRE-VPS PREFLIGHT PASSED\n'
printf 'VSIX: integrations/vscode/%s\n' "$OUT"
printf 'SHA256: integrations/vscode/%s.sha256\n' "$OUT"
printf 'Manifest: integrations/vscode/%s.release.json\n' "$OUT"
