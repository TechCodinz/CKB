#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXT="$ROOT/integrations/vscode"
cd "$EXT"

for tool in node npm git sha256sum unzip; do command -v "$tool" >/dev/null 2>&1 || { echo "$tool is required" >&2; exit 127; }; done

PUBLISHER="$(node -p "require('./package.json').publisher")"
NAME="$(node -p "require('./package.json').name")"
VERSION="$(node -p "require('./package.json').version")"
if [[ "$PUBLISHER" != 'TechCodinz' || "$NAME" != 'ckb-vscode' ]]; then
  echo "Unexpected extension identity: ${PUBLISHER}.${NAME}" >&2
  exit 2
fi

VSIX="ckb-vscode-${VERSION}.vsix"
CHECKSUM="${VSIX}.sha256"
MANIFEST="${VSIX}.release.json"
for required in "$VSIX" "$CHECKSUM" "$MANIFEST"; do
  [[ -f "$required" ]] || { echo "Missing verified release file: $required. Run scripts/release-stable-vscode.sh first." >&2; exit 3; }
done

sha256sum --check "$CHECKSUM"
unzip -t "$VSIX" >/dev/null
EXPECTED_SHA="$(awk '{print $1}' "$CHECKSUM")"
CURRENT_COMMIT="$(git -C "$ROOT" rev-parse HEAD)"
[[ -z "$(git -C "$ROOT" status --porcelain)" ]] || { echo 'Refusing Marketplace publication from a dirty checkout.' >&2; exit 4; }

node - "$MANIFEST" "$VERSION" "$VSIX" "$CURRENT_COMMIT" "$EXPECTED_SHA" <<'NODE'
const fs = require('fs');
const [manifestPath, version, artifact, currentCommit, expectedSha] = process.argv.slice(2);
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
const fail = msg => { console.error(msg); process.exit(5); };
if (manifest.schema !== 'ckb-vscode-release-manifest-v1') fail('Unexpected release manifest schema');
if (manifest.publisher !== 'TechCodinz' || manifest.extension !== 'ckb-vscode') fail('Release manifest identity mismatch');
if (manifest.version !== version || manifest.artifact !== artifact) fail('Release manifest version/artifact mismatch');
if (manifest.sourceDirty) fail('Release artifact was built from a dirty worktree');
if (manifest.sourceCommit !== currentCommit) fail(`Manifest source ${manifest.sourceCommit} != current checkout ${currentCommit}`);
if (String(manifest.sha256).toLowerCase() !== expectedSha.toLowerCase()) fail('Release manifest checksum mismatch');
NODE

if [[ "${CKB_INSTALL_TESTED_SHA256:-}" != "$EXPECTED_SHA" ]]; then
  echo "Refusing publish: set CKB_INSTALL_TESTED_SHA256=$EXPECTED_SHA only after manually installing/testing this exact VSIX." >&2
  exit 6
fi
if [[ "${CKB_CONFIRM_PUBLISH:-}" != "PUBLISH-${VERSION}" ]]; then
  echo "Refusing publish: set CKB_CONFIRM_PUBLISH=PUBLISH-${VERSION} after install verification." >&2
  exit 7
fi

PAT="${VSCE_PAT:-${VSCE_TOKEN:-}}"
[[ -n "$PAT" ]] || { echo 'Set VSCE_PAT (preferred) or VSCE_TOKEN in this shell. Never commit it.' >&2; exit 8; }

npx --no-install vsce publish --packagePath "$VSIX" --pat "$PAT"
echo "Published exact verified artifact: $VSIX"
echo "Marketplace identity: ${PUBLISHER}.${NAME} version ${VERSION}"
