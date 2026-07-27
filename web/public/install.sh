#!/bin/sh
set -e

# CKB Installer Script
# Detects OS and Architecture, downloads the latest binary from GitHub Releases,
# and installs it into ~/.local/bin or /usr/local/bin

REPO="yourusername/ckb-cli"
VERSION="latest"

echo "🚀 Installing CKB (Code Knowledge Base)..."

# Detect OS
OS="$(uname -s)"
case "${OS}" in
    Linux*)     OS_TARGET="linux";;
    Darwin*)    OS_TARGET="macos";;
    *)          echo "Unsupported OS: ${OS}"; exit 1;;
esac

# Detect Architecture
ARCH="$(uname -m)"
case "${ARCH}" in
    x86_64)     ARCH_TARGET="x86_64";;
    arm64|aarch64) ARCH_TARGET="aarch64";;
    *)          echo "Unsupported architecture: ${ARCH}"; exit 1;;
esac

# Determine OS specific binary suffix
SUFFIX=""
if [ "$OS_TARGET" = "linux" ]; then
    SUFFIX="unknown-linux-gnu"
elif [ "$OS_TARGET" = "macos" ]; then
    SUFFIX="apple-darwin"
fi

BINARY_NAME="ckb-${ARCH_TARGET}-${SUFFIX}.tar.gz"

# In a real release, fetch from GitHub API to get the latest version if VERSION=latest
# For this script, we'll construct the URL directly assuming standard GitHub release format
DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${BINARY_NAME}"

# Temp directory
TMP_DIR=$(mktemp -d)
cd "$TMP_DIR"

echo "⬇️  Downloading from ${DOWNLOAD_URL}..."
if curl -sL "$DOWNLOAD_URL" -o "ckb.tar.gz"; then
    tar -xzf ckb.tar.gz
    
    # Install location preference
    INSTALL_DIR="$HOME/.local/bin"
    if [ ! -d "$INSTALL_DIR" ]; then
        mkdir -p "$INSTALL_DIR"
    fi
    
    mv ckb "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/ckb"
    
    echo "✅ Successfully installed ckb to $INSTALL_DIR/ckb"
    
    # Check if INSTALL_DIR is in PATH
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *) echo "⚠️  Please add $INSTALL_DIR to your PATH in your .bashrc or .zshrc"
           echo 'export PATH="$HOME/.local/bin:$PATH"'
           ;;
    esac
    
    echo "🎉 Try running: ckb --help"
else
    echo "❌ Download failed or release not found. Check GitHub releases."
    exit 1
fi

# Cleanup
rm -rf "$TMP_DIR"
