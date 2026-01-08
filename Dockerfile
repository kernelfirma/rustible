# Rustible Multi-Stage Dockerfile
# Optimized for small image size and fast builds with caching
#
# Build: docker build -t rustible:latest .
# Run:   docker run --rm rustible:latest --version

# ============================================================================
# Stage 1: Build environment with cargo-chef for dependency caching
# ============================================================================
FROM rust:1.92-slim-bookworm AS chef

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-chef for optimized layer caching
RUN cargo install cargo-chef --locked

WORKDIR /app

# ============================================================================
# Stage 2: Prepare recipe (dependency list) for caching
# ============================================================================
FROM chef AS planner

# Copy source files needed to compute dependencies
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY benches ./benches

# Create the dependency recipe
RUN cargo chef prepare --recipe-path recipe.json

# ============================================================================
# Stage 3: Build dependencies (cached layer)
# ============================================================================
FROM chef AS builder

# Copy the recipe from planner
COPY --from=planner /app/recipe.json recipe.json

# Build dependencies only - this layer is cached unless Cargo.toml/Cargo.lock change
RUN cargo chef cook --release --recipe-path recipe.json --features "pure-rust"

# Now copy the full source and build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY benches ./benches

# Build the release binary with pure-rust features (no C dependencies)
RUN cargo build --release --features "pure-rust" --bin rustible

# Strip the binary to reduce size
RUN strip /app/target/release/rustible

# ============================================================================
# Stage 4: Minimal runtime image
# ============================================================================
FROM debian:13.2-slim AS runtime

# Install minimal runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    openssh-client \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -s /bin/bash rustible

# Copy the compiled binary
COPY --from=builder /app/target/release/rustible /usr/local/bin/rustible

# Create directories for playbooks and inventory
RUN mkdir -p /workspace /etc/rustible && \
    chown -R rustible:rustible /workspace /etc/rustible

# Set the working directory
WORKDIR /workspace

# Run as non-root user
USER rustible

# Default entrypoint
ENTRYPOINT ["/usr/local/bin/rustible"]
CMD ["--help"]

# Labels for container metadata
LABEL org.opencontainers.image.title="Rustible"
LABEL org.opencontainers.image.description="Fast, safe configuration management tool - Ansible alternative in Rust"
LABEL org.opencontainers.image.source="https://github.com/rustible/rustible"
LABEL org.opencontainers.image.licenses="MIT"
LABEL org.opencontainers.image.vendor="Rustible Contributors"
