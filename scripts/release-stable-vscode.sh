#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

need() { command -v "$1" >/dev/null 2>&1 || { echo "missing required tool: $1" >&2; exit 127; }; }
for tool in cargo node npm git sha256sum unzip; do need "$tool"; done

NODE_MAJOR="$(node -p "process.versions.node.split('.')[0]")"
if (( NODE_MAJOR < 18 )); then
  echo "Node 18+ is required (found $(node -v))" >&2
  exit 2
fi

if [[ -n "$(git status --porcelain)" && "${CKB_ALLOW_DIRTY_RELEASE:-0}" != "1" ]]; then
  echo 'Refusing to build a release artifact from a dirty worktree. Commit/stash changes first.' >&2
  exit 3
fi

printf '\n[1/5] Rust workspace check\n'
cargo check --workspace --all-targets

printf '\n[2/5] Rust workspace tests\n'
cargo test --workspace --all-targets

EXT="$ROOT/integrations/vscode"
pushd "$EXT" >/dev/null

printf '\n[3/5] Clean VS Code dependency install + TypeScript compile\n'
npm ci
npm run compile

PUBLISHER="$(node -p "require('./package.json').publisher")"
NAME="$(node -p "require('./package.json').name")"
VERSION="$(node -p "require('./package.json').version")"
if [[ "$PUBLISHER" != 'TechCodinz' || "$NAME" != 'ckb-vscode' ]]; then
  echo "Unexpected extension identity: ${PUBLISHER}.${NAME}" >&2
  exit 4
fi

VSIX="ckb-vscode-${VERSION}.vsix"
rm -f "$VSIX" "${VSIX}.sha256" "${VSIX}.release.json"

printf '\n[4/5] Package exact VSIX using lockfile-installed vsce\n'
npx --no-install vsce package --out "$VSIX"
unzip -t "$VSIX" >/dev/null
SHA256="$(sha256sum "$VSIX" | awk '{print $1}')"
printf '%s  %s\n' "$SHA256" "$VSIX" > "${VSIX}.sha256"
SOURCE_COMMIT="$(git -C "$ROOT" rev-parse HEAD)"
SOURCE_BRANCH="$(git -C "$ROOT" branch --show-current)"
SOURCE_DIRTY=false
[[ -n "$(git -C "$ROOT" status --porcelain)" ]] && SOURCE_DIRTY=true
if [[ "$SOURCE_DIRTY" == true && "${CKB_ALLOW_DIRTY_RELEASE:-0}" != "1" ]]; then
  echo 'Worktree became dirty during release preflight.' >&2
  exit 5
fi

node - "$VERSION" "$VSIX" "$SHA256" "$SOURCE_COMMIT" "$SOURCE_BRANCH" "$SOURCE_DIRTY" > "${VSIX}.release.json" <<'NODE'
const [version, artifact, sha256, sourceCommit, sourceBranch, sourceDirty] = process.argv.slice(2);
process.stdout.write(JSON.stringify({
  schema: 'ckb-vscode-release-manifest-v1',
  publisher: 'TechCodinz',
  extension: 'ckb-vscode',
  version,
  artifact,
  sha256,
  sourceCommit,
  sourceBranch,
  sourceDirty: sourceDirty === 'true',
  nodeVersion: process.version,
  generatedAt: new Date().toISOString(),
}, null, 2) + '\n');
NODE

printf '\n[5/5] Release artifact verified\n'
printf 'VSIX: %s/%s\nSHA256: %s\nManifest: %s/%s.release.json\n' "$EXT" "$VSIX" "$SHA256" "$EXT" "$VSIX"
printf '\nInstall-test this exact artifact before publication:\ncode --install-extension "%s/%s" --force\n' "$EXT" "$VSIX"
printf '\nThen set CKB_INSTALL_TESTED_SHA256=%s and CKB_CONFIRM_PUBLISH=PUBLISH-%s before running scripts/publish-stable-vscode.sh\n' "$SHA256" "$VERSION"

popd >/dev/null
