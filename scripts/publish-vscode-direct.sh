#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXT="$ROOT/integrations/vscode"
cd "$EXT"

command -v node >/dev/null || { echo 'Node.js is required' >&2; exit 127; }
command -v npm >/dev/null || { echo 'npm is required' >&2; exit 127; }
command -v git >/dev/null || { echo 'git is required' >&2; exit 127; }

NODE_MAJOR="$(node -p "process.versions.node.split('.')[0]")"
if (( NODE_MAJOR < 22 )); then
  echo "Node 22+ is required by current @vscode/vsce (found $(node -v))" >&2
  exit 2
fi

VERSION="$(node -p "require('./package.json').version")"
VSIX="ckb-vscode-${VERSION}.vsix"
CHECKSUM="${VSIX}.sha256"
MANIFEST="${VSIX}.release.json"

if [[ ! -f "$VSIX" || ! -f "$CHECKSUM" || ! -f "$MANIFEST" ]]; then
  echo "Verified release artifact/manifest missing. Run: bash scripts/preflight-v13.sh" >&2
  exit 3
fi

sha256sum --check "$CHECKSUM"
unzip -t "$VSIX" >/dev/null
CURRENT_COMMIT="$(git -C "$ROOT" rev-parse HEAD)"
node - "$MANIFEST" "$VERSION" "$VSIX" "$CURRENT_COMMIT" <<'NODE'
const fs = require('fs');
const [manifestPath, version, artifact, currentCommit] = process.argv.slice(2);
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
const fail = msg => { console.error(msg); process.exit(3); };
if (manifest.schema !== 'ckb-vscode-release-manifest-v1') fail('Unexpected release manifest schema');
if (manifest.publisher !== 'TechCodinz' || manifest.extension !== 'ckb-vscode') fail('Release manifest identity mismatch');
if (manifest.version !== version || manifest.artifact !== artifact) fail('Release manifest version/artifact mismatch');
if (manifest.sourceDirty) fail('Release manifest was produced from a dirty worktree');
if (manifest.sourceCommit !== currentCommit) fail(`Release manifest source ${manifest.sourceCommit} != current checkout ${currentCommit}`);
NODE

EXPECTED_SHA="$(cut -d' ' -f1 "$CHECKSUM")"
MANIFEST_SHA="$(node -p "require('./${MANIFEST}').sha256")"
if [[ "$EXPECTED_SHA" != "$MANIFEST_SHA" ]]; then
  echo 'Release manifest checksum does not match checksum file.' >&2
  exit 3
fi

if [[ -z "${VSCE_PAT:-${VSCE_TOKEN:-}}" ]]; then
  echo 'Set VSCE_PAT (preferred) or VSCE_TOKEN in this shell. Never commit it.' >&2
  exit 4
fi
PAT="${VSCE_PAT:-${VSCE_TOKEN}}"

if [[ "${CKB_CONFIRM_PUBLISH:-}" != "PUBLISH-${VERSION}" ]]; then
  echo "Refusing to publish without explicit confirmation." >&2
  echo "Set CKB_CONFIRM_PUBLISH=PUBLISH-${VERSION} after installing/testing this exact VSIX." >&2
  exit 5
fi

npx --yes @vscode/vsce@latest publish --packagePath "$VSIX" --pat "$PAT"
echo "Published exact verified artifact: $VSIX"
