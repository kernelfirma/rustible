# Quick Start Guide

Deploy your notebook in 5 minutes.

## Prerequisites

1. **EndeavourOS installed** on the notebook with:
   - User: `artur`
   - SSH enabled
   - Known IP address

2. **Rustible installed** on your workstation:
   ```bash
   cd /home/artur/Repositories/rustible
   cargo build --release
   sudo cp target/release/rustible /usr/local/bin/
   ```

## Deploy in 3 Steps

### Step 1: Configure

Edit `inventory.yml` and set your notebook's IP:

```bash
cd /home/artur/Repositories/rustible/deploy/notebook
vim inventory.yml
# Change: ansible_host: 192.168.1.100
```

### Step 2: Test Connection

```bash
# Check pre-requisites
./scripts/pre-deploy.sh

# Or test SSH manually
ssh artur@192.168.1.100
```

### Step 3: Deploy

```bash
# Dry run first (recommended)
make check

# Deploy everything
make deploy

# Or use rustible directly
rustible run playbook.yml -i inventory.yml
```

## Verify Deployment

```bash
# Run post-deployment checks
./scripts/post-deploy.sh

# Or SSH and check manually
ssh artur@192.168.1.100
echo $XDG_SESSION_TYPE  # Should be: wayland
ls -la ~/.config/hypr   # Should show symlinks
```

## Reboot & Enjoy

```bash
ssh artur@192.168.1.100 'sudo reboot'
```

After reboot, login via SDDM and enjoy your Hyprland setup!

## Common Commands

```bash
# Deploy specific parts only
make deploy-packages     # Just packages
make deploy-dotfiles     # Just dotfiles
make deploy-security     # Just security hardening

# View what will happen
make plan

# Debug with verbose output
make deploy-verbose

# SSH to notebook
make ssh
```

## Troubleshooting

**Connection refused?**
```bash
# SSH not configured
ssh-copy-id artur@192.168.1.100
```

**Permission denied?**
```bash
# Use password authentication
rustible run playbook.yml -i inventory.yml -k
```

**Packages failing?**
```bash
# Update mirrors on the notebook
ssh artur@192.168.1.100
sudo reflector --country Austria,Germany --latest 20 --sort rate --save /etc/pacman.d/mirrorlist
```

## Next Steps

- Set wallpaper: `hyprctl hyprpaper preload /path/to/wall.jpg`
- Login to Bitwarden: `bw login your@email.com`
- Setup Tailscale: `sudo tailscale up`
- Configure backup: Edit `~/bin/backup-to-nas.sh`

For full documentation, see [README.md](README.md).
