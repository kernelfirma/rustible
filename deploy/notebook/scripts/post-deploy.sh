#!/bin/bash
# Post-deployment verification script

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

# Get notebook IP
NOTEBOOK_IP=$(grep "ansible_host:" "$PROJECT_DIR/inventory.yml" | head -1 | awk '{print $2}')
SSH_CMD="ssh -o ConnectTimeout=10 artur@$NOTEBOOK_IP"

info "Running post-deployment verification on $NOTEBOOK_IP..."
info ""

# Check system basics
info "Checking system basics..."
$SSH_CMD "hostname" || { error "Cannot connect to notebook"; exit 1; }

# Check user
info "Checking user configuration..."
$SSH_CMD "id" | grep -q "artur" && info "  ✓ User exists" || error "  ✗ User not found"
$SSH_CMD "groups" | grep -q "wheel" && info "  ✓ User in wheel group" || warn "  ✗ User not in wheel group"

# Check packages
info "Checking critical packages..."
PACKAGES=("hyprland" "wezterm" "yazi" "paru" "git" "pipewire")
for pkg in "${PACKAGES[@]}"; do
    if $SSH_CMD "pacman -Q $pkg" &>/dev/null; then
        info "  ✓ $pkg installed"
    else
        warn "  ✗ $pkg not found"
    fi
done

# Check dotfiles
info "Checking dotfiles..."
DOTFILES=(".config/hypr/hyprland.conf" ".config/waybar/config.jsonc" ".vimrc" ".config/yazi/yazi.toml")
for file in "${DOTFILES[@]}"; do
    if $SSH_CMD "test -L /home/artur/$file" &>/dev/null; then
        info "  ✓ $file linked"
    elif $SSH_CMD "test -f /home/artur/$file" &>/dev/null; then
        warn "  ⚠ $file exists but is not a symlink"
    else
        error "  ✗ $file missing"
    fi
done

# Check services
info "Checking services..."
SERVICES=("sshd" "NetworkManager" "bluetooth")
for svc in "${SERVICES[@]}"; do
    if $SSH_CMD "systemctl is-active $svc" &>/dev/null; then
        info "  ✓ $svc running"
    else
        warn "  ✗ $svc not running"
    fi
done

# Summary
info ""
info "======================================"
info "Post-deployment verification complete!"
info "======================================"
info ""
info "Next steps:"
info "  1. Reboot: ssh artur@$NOTEBOOK_IP 'sudo reboot'"
info "  2. Login to Hyprland via SDDM"
info "  3. Configure wallpaper: hyprctl hyprpaper preload <image>"
info ""
