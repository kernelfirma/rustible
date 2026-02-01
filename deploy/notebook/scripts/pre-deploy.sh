#!/bin/bash
# Pre-deployment checks and setup

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; }

info "Running pre-deployment checks..."

# Check if rustible is installed
if ! command -v rustible &> /dev/null; then
    error "rustible not found in PATH"
    error "Please build and install rustible:"
    error "  cd /home/artur/Repositories/rustible && cargo build --release"
    exit 1
fi

RUSTIBLE_VERSION=$(rustible --version 2>/dev/null || echo "unknown")
info "Found rustible: $RUSTIBLE_VERSION"

# Check if inventory file exists
if [ ! -f "$PROJECT_DIR/inventory.yml" ]; then
    error "inventory.yml not found"
    exit 1
fi

# Extract notebook IP from inventory
NOTEBOOK_IP=$(grep "ansible_host:" "$PROJECT_DIR/inventory.yml" | head -1 | awk '{print $2}')
info "Target notebook IP: $NOTEBOOK_IP"

# Check if notebook is reachable
info "Checking network connectivity to $NOTEBOOK_IP..."
if ! ping -c 1 -W 2 "$NOTEBOOK_IP" &> /dev/null; then
    warn "Notebook at $NOTEBOOK_IP is not responding to ping"
    warn "Continuing anyway..."
else
    info "Notebook is reachable"
fi

# Check SSH connectivity
info "Checking SSH connectivity..."
if ! ssh -o ConnectTimeout=5 -o BatchMode=yes "$NOTEBOOK_IP" exit 2>/dev/null; then
    warn "SSH key authentication not configured"
    warn "Run: ssh-copy-id artur@$NOTEBOOK_IP"
    warn "Or use: rustible run playbook.yml -i inventory.yml -k (for password auth)"
else
    info "SSH connection successful"
fi

# Summary
info ""
info "Pre-deployment checks complete!"
info ""
info "To deploy, run:"
info "  cd $PROJECT_DIR"
info "  rustible run playbook.yml -i inventory.yml"
info ""
