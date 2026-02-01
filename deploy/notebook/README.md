# Notebook Deployment Guide

Deploy a fresh notebook with Arch Linux using Rustible - your dotfiles, packages, and full Hyprland setup in one command.

## Prerequisites

### 1. Install EndeavourOS on the notebook

1. Download ISO: https://endeavouros.com/
2. Create bootable USB: `sudo dd if=endeavouros.iso of=/dev/sdX bs=4M status=progress`
3. Boot from USB and run the Calamares installer
4. Installation settings:
   - **Profile**: Minimal (no desktop environment)
   - **User**: `artur` (must match ansible_user in inventory)
   - **Enable SSH**: Check the box during installation
   - **Partitioning**: Automatic or manual as preferred

### 2. Prepare the Control Machine (Your Main Workstation)

Ensure Rustible is built and available:

```bash
cd /home/artur/Repositories/rustible
cargo build --release
sudo cp target/release/rustible /usr/local/bin/

# Verify installation
rustible --version
```

### 3. Configure SSH Access

```bash
# Copy your SSH key to the notebook
ssh-copy-id artur@192.168.1.100  # Update IP as needed

# Test connection
ssh artur@192.168.1.100
```

### 4. Update Inventory

Edit `inventory.yml` and set the correct IP address:

```bash
vim /home/artur/Repositories/rustible/deploy/notebook/inventory.yml
# Change: ansible_host: 192.168.1.100  → Your notebook's IP
```

### 5. Update Dotfiles Repository URL

Edit `group_vars/all.yml`:

```bash
vim /home/artur/Repositories/rustible/deploy/notebook/group_vars/all.yml
# Change: dotfiles_repo_url: https://github.com/YOUR_USERNAME/dotfiles.git
```

## Quick Start

```bash
cd /home/artur/Repositories/rustible/deploy/notebook

# 1. Preview what will be done (dry run)
rustible run playbook.yml -i inventory.yml --check

# 2. See execution plan
rustible run playbook.yml -i inventory.yml --plan

# 3. Deploy everything
rustible run playbook.yml -i inventory.yml

# 4. Deploy specific roles only
rustible run playbook.yml -i inventory.yml --tags packages,dotfiles

# 5. Deploy with verbose output
rustible run playbook.yml -i inventory.yml -vvv
```

## Role Descriptions

| Role | Purpose | Tags |
|------|---------|------|
| `arch-setup` | Initialize Arch system (pacman, locale, time, SSH) | `arch`, `base` |
| `users` | Create user, groups, sudo configuration | `users` |
| `packages` | Install core and laptop-specific packages | `packages` |
| `dotfiles` | Deploy dotfiles repository and create symlinks | `dotfiles` |
| `graphics` | Configure graphics drivers, display manager, audio | `graphics` |
| `system` | Enable services, automatic updates, backups | `system` |
| `security` | Hardening with UFW, Fail2ban, SSH security | `security` |

## Post-Deployment

After running the playbook:

### 1. Reboot the Notebook

```bash
ssh artur@192.168.1.100
sudo reboot
```

### 2. Login and Verify

```bash
# Check Hyprland session
echo $XDG_SESSION_TYPE  # Should output: wayland

# Verify dotfiles
ls -la ~/.config/hypr
ls -la ~/.vimrc

# Test services
systemctl --user status pipewire
systemctl --user status wireplumber
```

### 3. Manual Configuration Steps

```bash
# Set wallpaper in Hyprpaper
hyprctl hyprpaper preload /path/to/wallpaper.jpg
hyprctl hyprpaper wallpaper "eDP-1,/path/to/wallpaper.jpg"

# Configure Bitwarden
bw login your@email.com

# Setup Tailscale (if not auto-configured)
sudo tailscale up

# Setup NordVPN (if enabled)
nordvpn login
```

## Customization

### Add New Packages

Edit package lists in your dotfiles repository:

```bash
vim ~/Repositories/dotfiles/packages/core.txt
vim ~/Repositories/dotfiles/packages/laptop.txt
```

Then re-run the packages role:

```bash
rustible run playbook.yml -i inventory.yml --tags packages
```

### Modify Role Variables

```bash
vim inventory.yml          # Host-specific settings
vim group_vars/all.yml     # Common settings
vim host_vars/notebook.yml # Notebook-specific settings
```

### Add New Dotfiles

1. Add files to `~/Repositories/dotfiles/config/`
2. Update `roles/dotfiles/tasks/main.yml` to link them
3. Commit and push changes
4. Re-run the dotfiles role:
   ```bash
   rustible run playbook.yml -i inventory.yml --tags dotfiles
   ```

## Troubleshooting

### SSH Connection Issues

```bash
# Add notebook SSH key to known hosts
ssh-keyscan 192.168.1.100 >> ~/.ssh/known_hosts

# Test connection
ssh artur@192.168.1.100

# If SSH key not accepted, use password auth temporarily
rustible run playbook.yml -i inventory.yml -k
```

### Dotfiles Not Linking

```bash
# Check dotfiles repository
ls -la ~/Repositories/dotfiles

# Manually run install script on the notebook
ssh artur@192.168.1.100
cd ~/Repositories/dotfiles
./install.sh
```

### Package Installation Fails

```bash
# On the notebook, update mirrors
sudo reflector --country Austria,Germany --latest 20 --sort rate --save /etc/pacman.d/mirrorlist

# Clear package cache
sudo pacman -Sc

# Update system
sudo pacman -Syu
```

### Hyprland Won't Start

```bash
# Check logs
journalctl -xe | grep hyprland

# Verify environment
echo $XDG_SESSION_TYPE
echo $WAYLAND_DISPLAY

# Test configuration file
hyprctl -c ~/.config/hypr/hyprland.conf check 2>&1 || echo "Config error"

# Check if host-specific config exists
ls -la ~/.config/hypr/host.conf
```

### Permission Issues

```bash
# Fix home directory permissions
sudo chown -R artur:artur /home/artur

# Fix .config permissions
sudo chown -R artur:artur /home/artur/.config
sudo chown -R artur:artur /home/artur/bin
```

### Audio Not Working

```bash
# Check PipeWire status
systemctl --user status pipewire
systemctl --user status pipewire-pulse
systemctl --user status wireplumber

# Restart audio services
systemctl --user restart pipewire pipewire-pulse wireplumber

# Check audio devices
pactl info
pactl list sinks
```

## Maintenance

### Update System

```bash
# Manual update on the notebook
sudo pacman -Syu

# Or run the system role to reconfigure
rustible run playbook.yml -i inventory.yml --tags system
```

### Update Dotfiles

```bash
# On the notebook
cd ~/Repositories/dotfiles
git pull

# Re-run dotfiles role if needed
rustible run playbook.yml -i inventory.yml --tags dotfiles
```

### Backup

```bash
# Manual backup on the notebook
~/bin/backup-to-nas.sh

# Check backup cron
crontab -l | grep backup

# View backup logs
journalctl -t backup-to-nas.sh
```

### Add Another Notebook

1. Copy the inventory:
   ```bash
   cp host_vars/notebook.yml host_vars/notebook2.yml
   ```

2. Update the new host file with new IP/settings

3. Add to inventory.yml:
   ```yaml
   notebook2:
     ansible_host: 192.168.1.101
     machine_type: laptop
   ```

4. Deploy:
   ```bash
   rustible run playbook.yml -i inventory.yml --limit notebook2
   ```

## Architecture

```
deploy/notebook/
├── playbook.yml              # Main orchestration
├── inventory.yml             # Host definitions
├── group_vars/
│   └── all.yml              # Common variables
├── host_vars/
│   └── notebook.yml         # Host-specific variables
└── roles/
    ├── arch-setup/          # System initialization
    ├── users/               # User/group management
    ├── packages/            # Package installation
    ├── dotfiles/            # Dotfiles deployment
    ├── graphics/            # GPU/display/audio
    ├── system/              # Services & automation
    └── security/            # Hardening
```

## Security Notes

- SSH root login is disabled
- Password authentication is disabled (key-based only)
- UFW firewall is enabled with default deny
- Fail2ban is enabled for SSH protection
- Automatic security updates are configured

## Performance

This deployment uses Rustible for:
- **5-11x faster** execution vs Ansible
- **Automatic connection pooling** for SSH
- **Parallel execution** of independent tasks
- **Native Rust modules** where available

## Support

- **Rustible Docs**: https://github.com/ruvnet/rustible
- **EndeavourOS Wiki**: https://discovery.endeavouros.com/
- **Arch Wiki**: https://wiki.archlinux.org/
- **Hyprland Wiki**: https://wiki.hyprland.org/

## License

MIT - Same as Rustible project
