---
summary: Comprehensive CLI documentation covering all commands (run, check, vault, list-hosts, validate, init), options, and environment variables.
read_when: You need to look up CLI commands, options, or configuration settings.
---

# Rustible CLI Reference

This document provides comprehensive documentation for all Rustible CLI commands, options, and usage patterns.

## Global Options

These options can be used with any command:

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--inventory <PATH>` | `-i` | Path to inventory file or directory | `$RUSTIBLE_INVENTORY` |
| `--extra-vars <VARS>` | `-e` | Extra variables (key=value or @file.yml) | - |
| `--verbose` | `-v` | Increase verbosity (-v, -vv, -vvv, -vvvv) | 0 |
| `--check` | - | Run in check mode (dry-run) | false |
| `--diff` | - | Show differences when files change | false |
| `--output <FORMAT>` | - | Output format: human, json, yaml, minimal | human |
| `--limit <PATTERN>` | `-l` | Limit execution to specific hosts | - |
| `--forks <N>` | `-f` | Number of parallel processes | 5 |
| `--timeout <SECS>` | - | Connection timeout in seconds | 30 |
| `--config <PATH>` | `-c` | Path to configuration file | `$RUSTIBLE_CONFIG` |
| `--no-color` | - | Disable colored output | false |

---

## rustible run

Execute a playbook against target hosts.

### Synopsis

```
rustible run [OPTIONS] <PLAYBOOK>
```

### Description

The `run` command executes an Ansible-compatible playbook against the specified inventory. It supports tags, step-by-step execution, vault integration, privilege escalation, and custom SSH options.

### Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `<PLAYBOOK>` | - | Path to the playbook file (required) | - |
| `--tags <TAGS>` | `-t` | Only run tasks with these tags (can be repeated) | - |
| `--skip-tags <TAGS>` | - | Skip tasks with these tags (can be repeated) | - |
| `--start-at-task <NAME>` | - | Start execution at the specified task | - |
| `--step` | - | Step through tasks one at a time | false |
| `--plan` | - | Show execution plan without running | false |
| `--ask-vault-pass` | - | Prompt for vault password | false |
| `--vault-password-file <PATH>` | - | File containing vault password | - |
| `--become` | `-b` | Enable privilege escalation | false |
| `--become-method <METHOD>` | - | Privilege escalation method | sudo |
| `--become-user <USER>` | - | User for privilege escalation | root |
| `--ask-become-pass` | `-K` | Prompt for become password | false |
| `--user <USER>` | `-u` | Remote SSH user | current user |
| `--private-key <PATH>` | - | Path to SSH private key | - |
| `--ssh-common-args <ARGS>` | - | Additional SSH arguments | - |

### Examples

**Basic playbook execution:**
```bash
rustible run site.yml
```

**Execute with inventory and become:**
```bash
rustible run -i inventory.yml site.yml -b --become-user root
```

**Run only specific tags with variables:**
```bash
rustible run playbook.yml -t deploy -t configure -e "version=1.2.3" -e "env=production"
```

**Preview execution plan:**
```bash
rustible run playbook.yml --plan
```

**Step through tasks interactively:**
```bash
rustible run playbook.yml --step --start-at-task "Install packages"
```

**Use vault-encrypted variables:**
```bash
rustible run playbook.yml --vault-password-file ~/.vault_pass
```

### Exit Codes

| Code | Description |
|------|-------------|
| 0 | Success - all tasks completed without errors |
| 1 | Error - playbook not found, syntax error, or configuration issue |
| 2 | Failure - one or more tasks failed on one or more hosts |

---

## rustible check

Run a playbook in check mode (dry-run) without making changes.

### Synopsis

```
rustible check [OPTIONS] <PLAYBOOK>
```

### Description

The `check` command performs a dry-run of the playbook, showing what changes would be made without actually executing them. This is useful for validating playbooks and previewing changes before deployment.

### Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `<PLAYBOOK>` | - | Path to the playbook file (required) | - |
| `--tags <TAGS>` | `-t` | Only check tasks with these tags | - |
| `--skip-tags <TAGS>` | - | Skip tasks with these tags | - |
| `--start-at-task <NAME>` | - | Start at the specified task | - |
| `--ask-vault-pass` | - | Prompt for vault password | false |
| `--vault-password-file <PATH>` | - | File containing vault password | - |
| `--become` | `-b` | Enable privilege escalation | false |
| `--become-method <METHOD>` | - | Privilege escalation method | sudo |
| `--become-user <USER>` | - | User for privilege escalation | root |
| `--user <USER>` | `-u` | Remote SSH user | current user |
| `--private-key <PATH>` | - | Path to SSH private key | - |

### Examples

**Basic syntax check:**
```bash
rustible check playbook.yml
```

**Check with diff output:**
```bash
rustible check playbook.yml --diff
```

**Check specific hosts only:**
```bash
rustible check -i inventory.yml -l webservers playbook.yml
```

### Exit Codes

| Code | Description |
|------|-------------|
| 0 | Success - playbook syntax is valid |
| 1 | Error - playbook not found or syntax error |
| 2 | Failure - tasks would fail if executed |

---

## rustible vault

Manage encrypted secrets using AES-256-GCM encryption.

### Synopsis

```
rustible vault <SUBCOMMAND> [OPTIONS]
```

### Description

The `vault` command provides encryption and decryption capabilities for sensitive data. Rustible uses AES-256-GCM encryption with Argon2 key derivation, providing strong protection for secrets.

### Subcommands

#### vault encrypt

Encrypt a plaintext file.

```
rustible vault encrypt [OPTIONS] <FILE>
```

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `<FILE>` | - | File to encrypt (required) | - |
| `--output <PATH>` | `-o` | Output file (overwrites input if not specified) | - |
| `--vault-password-file <PATH>` | - | File containing vault password | - |
| `--vault-id <ID>` | - | Vault ID for multi-vault setups | - |

**Example:**
```bash
rustible vault encrypt secrets.yml
rustible vault encrypt secrets.yml -o secrets.yml.enc
```

#### vault decrypt

Decrypt an encrypted file.

```
rustible vault decrypt [OPTIONS] <FILE>
```

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `<FILE>` | - | File to decrypt (required) | - |
| `--output <PATH>` | `-o` | Output file (overwrites input if not specified) | - |
| `--vault-password-file <PATH>` | - | File containing vault password | - |

**Example:**
```bash
rustible vault decrypt secrets.yml
rustible vault decrypt secrets.yml -o secrets.yml.plain
```

#### vault view

View contents of an encrypted file without decrypting to disk.

```
rustible vault view [OPTIONS] <FILE>
```

| Option | Description |
|--------|-------------|
| `<FILE>` | File to view (required) |
| `--vault-password-file <PATH>` | File containing vault password |

**Example:**
```bash
rustible vault view secrets.yml
rustible vault view secrets.yml --vault-password-file ~/.vault_pass
```

#### vault edit

Edit an encrypted file in-place using your default editor.

```
rustible vault edit [OPTIONS] <FILE>
```

| Option | Description | Default |
|--------|-------------|---------|
| `<FILE>` | File to edit (required) | - |
| `--vault-password-file <PATH>` | File containing vault password | - |
| `--editor <EDITOR>` | Editor to use | `$EDITOR` or vi |

**Example:**
```bash
rustible vault edit secrets.yml
EDITOR=nano rustible vault edit secrets.yml
```

#### vault create

Create a new encrypted file.

```
rustible vault create [OPTIONS] <FILE>
```

| Option | Description | Default |
|--------|-------------|---------|
| `<FILE>` | File to create (required) | - |
| `--vault-password-file <PATH>` | File containing vault password | - |
| `--editor <EDITOR>` | Editor to use | `$EDITOR` or vi |

**Example:**
```bash
rustible vault create new-secrets.yml
```

#### vault rekey

Change the encryption password for one or more files.

```
rustible vault rekey [OPTIONS] <FILES>...
```

| Option | Description |
|--------|-------------|
| `<FILES>...` | Files to rekey (required, multiple allowed) |
| `--vault-password-file <PATH>` | Current vault password file |
| `--new-vault-password-file <PATH>` | New vault password file |

**Example:**
```bash
rustible vault rekey secrets.yml credentials.yml
rustible vault rekey --vault-password-file old.pass --new-vault-password-file new.pass secrets.yml
```

#### vault encrypt-string

Encrypt a string value for embedding in YAML files.

```
rustible vault encrypt-string [OPTIONS] [STRING]
```

| Option | Short | Description |
|--------|-------|-------------|
| `[STRING]` | - | String to encrypt (reads from stdin if not provided) |
| `--stdin-name <NAME>` | `-p` | Variable name for YAML output |
| `--vault-password-file <PATH>` | - | File containing vault password |

**Example:**
```bash
rustible vault encrypt-string "my_secret_password"
rustible vault encrypt-string -p db_password "s3cr3t"
echo "password" | rustible vault encrypt-string
```

#### vault decrypt-string

Decrypt a vault-encrypted string.

```
rustible vault decrypt-string [OPTIONS] <STRING>
```

| Option | Description |
|--------|-------------|
| `<STRING>` | Encrypted string to decrypt (required) |
| `--vault-password-file <PATH>` | File containing vault password |

**Example:**
```bash
rustible vault decrypt-string '$RUSTIBLE_VAULT;1.0;AES256-GCM...'
```

### Exit Codes

| Code | Description |
|------|-------------|
| 0 | Success |
| 1 | Error - file not found, wrong password, or encryption failure |

---

## rustible list-hosts

List hosts matching a pattern from the inventory.

### Synopsis

```
rustible list-hosts [OPTIONS] [PATTERN]
```

### Description

The `list-hosts` command displays hosts from the inventory that match the specified pattern. It can show host variables and group relationships.

### Options

| Option | Description | Default |
|--------|-------------|---------|
| `[PATTERN]` | Host pattern to match | all |
| `--vars` | Show host variables | false |
| `--yaml` | Output as YAML | false |
| `--graph` | Show hosts grouped by group membership | false |

### Examples

**List all hosts:**
```bash
rustible list-hosts -i inventory.yml
```

**List hosts in a specific group:**
```bash
rustible list-hosts -i inventory.yml webservers
```

**Show hosts with their variables:**
```bash
rustible list-hosts -i inventory.yml --vars
```

**Display as inventory graph:**
```bash
rustible list-hosts -i inventory.yml --graph
```

**YAML output for scripting:**
```bash
rustible list-hosts -i inventory.yml --yaml
```

### Exit Codes

| Code | Description |
|------|-------------|
| 0 | Success |
| 1 | Error - no inventory specified or file not found |

---

## rustible list-tasks

List tasks in a playbook.

### Synopsis

```
rustible list-tasks [OPTIONS] <PLAYBOOK>
```

### Description

The `list-tasks` command displays all tasks defined in a playbook, including pre-tasks, post-tasks, and handlers.

### Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `<PLAYBOOK>` | - | Path to playbook file (required) | - |
| `--tags <TAGS>` | `-t` | Show only tasks with these tags | - |
| `--skip-tags <TAGS>` | - | Skip tasks with these tags | - |
| `--detailed` | - | Include task details (module, conditions) | false |

### Examples

**List all tasks:**
```bash
rustible list-tasks playbook.yml
```

**List tasks with specific tags:**
```bash
rustible list-tasks playbook.yml -t deploy
```

**Show detailed task information:**
```bash
rustible list-tasks playbook.yml --detailed
```

### Exit Codes

| Code | Description |
|------|-------------|
| 0 | Success |
| 1 | Error - playbook not found or syntax error |

---

## rustible validate

Validate playbook syntax without execution.

### Synopsis

```
rustible validate <PLAYBOOK>
```

### Description

The `validate` command checks playbook syntax and structure without connecting to any hosts or making changes. It verifies YAML syntax and playbook structure.

### Options

| Option | Description |
|--------|-------------|
| `<PLAYBOOK>` | Path to playbook file (required) |

### Examples

**Validate a playbook:**
```bash
rustible validate site.yml
```

**Validate with verbose output:**
```bash
rustible validate -v playbook.yml
```

### Exit Codes

| Code | Description |
|------|-------------|
| 0 | Success - playbook syntax is valid |
| 1 | Error - playbook not found or contains syntax errors |

---

## rustible init

Initialize a new Rustible project.

### Synopsis

```
rustible init [OPTIONS] [PATH]
```

### Description

The `init` command creates a new Rustible project structure with default configuration files, directory layout, and example playbooks.

### Options

| Option | Description | Default |
|--------|-------------|---------|
| `[PATH]` | Directory to initialize | current directory |
| `--template <TEMPLATE>` | Project template to use | basic |

### Examples

**Initialize in current directory:**
```bash
rustible init
```

**Initialize in a specific directory:**
```bash
rustible init my-project
```

**Use a specific template:**
```bash
rustible init --template advanced my-project
```

### Exit Codes

| Code | Description |
|------|-------------|
| 0 | Success |
| 1 | Error - directory exists or permission denied |

---

## Environment Variables

Rustible respects the following environment variables:

| Variable | Description |
|----------|-------------|
| `RUSTIBLE_INVENTORY` | Default inventory file path |
| `RUSTIBLE_CONFIG` | Default configuration file path |
| `RUSTIBLE_VAULT_PASSWORD` | Vault password (use with caution) |
| `RUSTIBLE_VAULT_PASSWORD_FILE` | Path to vault password file |
| `RUSTIBLE_HOME` | Rustible home directory |
| `RUSTIBLE_REMOTE_USER` | Default remote SSH user |
| `RUSTIBLE_SSH_KEY` | Default SSH private key path |
| `RUSTIBLE_NO_COLOR` | Disable colored output |
| `NO_COLOR` | Standard no-color environment variable |
| `EDITOR` | Editor for vault edit/create commands |

---

## Limit Patterns

The `--limit` option supports various patterns:

| Pattern | Description | Example |
|---------|-------------|---------|
| `hostname` | Single host | `--limit web01` |
| `group` | All hosts in group | `--limit webservers` |
| `host1:host2` | Multiple hosts | `--limit web01:web02` |
| `group1:group2` | Multiple groups | `--limit webservers:dbservers` |
| `~regex` | Regex pattern | `--limit ~web.*` |
| `!pattern` | Exclude pattern | `--limit 'all:!dbservers'` |
| `&pattern` | Intersection | `--limit 'webservers:&production'` |
| `@filename` | Hosts from file | `--limit @hosts.txt` |

---

## Configuration File

Rustible looks for configuration in these locations (in order):

1. Path specified by `--config` or `$RUSTIBLE_CONFIG`
2. `./rustible.yml` (current directory)
3. `~/.rustible/config.yml`
4. `/etc/rustible/rustible.yml`

### Example Configuration

```yaml
defaults:
  inventory: ./inventory/hosts.yml
  forks: 10
  timeout: 30
  remote_user: deploy

ssh:
  private_key: ~/.ssh/id_ed25519
  common_args: "-o StrictHostKeyChecking=accept-new"

privilege_escalation:
  become: true
  become_method: sudo
  become_user: root
```

---

## See Also

- [Getting Started Guide](./getting-started.md)
- [Playbook Syntax](./playbook-syntax.md)
- [Inventory Format](./inventory-format.md)
- [Module Reference](./modules.md)
