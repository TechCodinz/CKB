# Multi-stage build for enterprise on-premise deployment
FROM rust:1.76-slim as builder

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./
COPY core/Cargo.toml ./core/
COPY cli/Cargo.toml ./cli/
COPY mcp-server/Cargo.toml ./mcp-server/

# Create dummy main files for dependency caching
RUN mkdir -p core/src && echo "fn main() {}" > core/src/lib.rs
RUN mkdir -p cli/src && echo "fn main() {}" > cli/src/main.rs
RUN mkdir -p mcp-server/src && echo "fn main() {}" > mcp-server/src/main.rs

# Build dependencies (cached layer)
RUN cargo build --release

# Copy actual source
COPY core ./core
COPY cli ./cli
COPY mcp-server ./mcp-server

# Touch files to force rebuild
RUN touch core/src/lib.rs cli/src/main.rs mcp-server/src/main.rs

# Build for release
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create ckb user
RUN useradd -m -s /bin/bash ckb

WORKDIR /app

# Copy binaries
COPY --from=builder /app/target/release/ckb-cli /usr/local/bin/ckb
COPY --from=builder /app/target/release/ckb-mcp-server /usr/local/bin/ckb-mcp

# Copy web dashboard
COPY web/build /app/web

# Create data directory
RUN mkdir -p /data/ckb && chown -R ckb:ckb /data /app

USER ckb

# Expose ports
EXPOSE 3000 3001

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD curl -f http://localhost:3000/health || exit 1

# Default command
CMD ["ckb-mcp", "--host", "0.0.0.0", "--port", "3000"]
