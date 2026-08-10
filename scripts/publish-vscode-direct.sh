#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXT="$ROOT/integrations/vscode"
cd "$EXT"

command -v node >/dev/null || { echo 'Node.js is required' >&2; exit 127; }
command -v npm >/dev/null || { echo 'npm is required' >&2; exit 127; }

NODE_MAJOR="$(node -p "process.versions.node.split('.')[0]")"
if (( NODE_MAJOR < 22 )); then
  echo "Node 22+ is required by current @vscode/vsce (found $(node -v))" >&2
  exit 2
fi

VERSION="$(node -p "require('./package.json').version")"
VSIX="ckb-vscode-${VERSION}.vsix"
CHECKSUM="${VSIX}.sha256"

if [[ ! -f "$VSIX" || ! -f "$CHECKSUM" ]]; then
  echo "Verified release artifact missing. Run: bash scripts/preflight-v13.sh" >&2
  exit 3
fi

sha256sum --check "$CHECKSUM"
unzip -t "$VSIX" >/dev/null

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
