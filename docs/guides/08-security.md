---
summary: Vault encryption (AES-256-GCM with Argon2), SSH key management, privilege escalation, and security best practices.
read_when: You need to encrypt secrets, manage SSH connections securely, or configure privilege escalation.
---

# Chapter 8: Security - Vault, SSH, and Privilege Escalation

Security is a first-class concern in Rustible. This chapter covers the vault encryption system for protecting secrets, SSH connection security, privilege escalation, and general security hardening practices.

## Vault Encryption

Rustible Vault encrypts sensitive data using **AES-256-GCM** (authenticated encryption) with **Argon2** key derivation. This is a modern, memory-hard key derivation function that resists brute-force and GPU-based attacks.

Encrypted files are identified by the header:

```
$RUSTIBLE_VAULT;1.0;AES256-GCM
```

Vault passwords are stored in memory using `SecretString`, which automatically zeroes memory on drop to prevent secret leakage.

### Vault Commands

| Command | Description |
|---------|-------------|
| `vault encrypt` | Encrypt an existing file |
| `vault decrypt` | Decrypt an encrypted file |
| `vault view` | View encrypted file contents without decrypting to disk |
| `vault edit` | Decrypt, open in editor, re-encrypt on save |
| `vault create` | Create a new encrypted file |
| `vault rekey` | Re-encrypt with a new password |
| `vault encrypt-string` | Encrypt a single string value |
| `vault decrypt-string` | Decrypt a single encrypted string |
| `vault init` | Initialize a new vault password file |

### Encrypting Files

```bash
# Encrypt a file (prompts for password)
rustible vault encrypt secrets.yml

# Encrypt with a password file
rustible vault encrypt secrets.yml --vault-password-file .vault_pass

# Encrypt to a different output file
rustible vault encrypt secrets.yml -O secrets.enc.yml
```

### Decrypting Files

```bash
# Decrypt a file
rustible vault decrypt secrets.enc.yml --vault-password-file .vault_pass

# View without writing to disk
rustible vault view secrets.enc.yml --vault-password-file .vault_pass
```

### Editing Encrypted Files

```bash
# Opens in $EDITOR, re-encrypts on save
rustible vault edit secrets.yml --vault-password-file .vault_pass

# Specify editor explicitly
rustible vault edit secrets.yml --editor nano --vault-password-file .vault_pass
```

### Re-keying

Change the encryption password without exposing the plaintext:

```bash
rustible vault rekey secrets.yml --vault-password-file .vault_pass
```

### Encrypting Inline Strings

Encrypt individual values for embedding directly in YAML files:

```bash
rustible vault encrypt-string "SuperSecret123" --vault-password-file .vault_pass
```

This outputs an encrypted string that you can paste into a variable file:

```yaml
database_password: !vault |
  $RUSTIBLE_VAULT;1.0;AES256-GCM
  <base64-encoded-encrypted-data>
```

### Using Vault in Playbooks

Provide the vault password when running playbooks:

```bash
# Prompt for password
rustible run playbook.yml --ask-vault-pass

# Use a password file
rustible run playbook.yml --vault-password-file .vault_pass

# Password file can be a script that outputs the password
rustible run playbook.yml --vault-password-file ./get-vault-pass.sh
```

### Multi-Vault Support

Use vault IDs to manage multiple vaults with different passwords:

```bash
# Encrypt with a vault ID
rustible vault encrypt secrets.yml --vault-id prod@.vault_pass_prod

# Use multiple vault IDs at runtime
rustible run playbook.yml \
  --vault-id dev@.vault_pass_dev \
  --vault-id prod@.vault_pass_prod
```

This allows different teams or environments to use separate encryption passwords.

## SSH Key Management

### Key Types

Rustible supports standard SSH key types through the `russh` library:

- **Ed25519** (recommended): fast, small keys, strong security
- **RSA** (2048-bit minimum, 4096-bit recommended): wide compatibility
- **ECDSA**: elliptic curve keys

### Specifying Keys

Set the SSH key per host or group:

```yaml
# In inventory
all:
  children:
    production:
      vars:
        ansible_ssh_private_key_file: ~/.ssh/production_ed25519
      hosts:
        web1:
          ansible_host: 10.0.0.1
```

Or on the command line:

```bash
rustible run playbook.yml --private-key ~/.ssh/production_ed25519
```

### SSH Agent Forwarding

For hosts that need to access Git repositories or other SSH-authenticated resources:

```yaml
- hosts: webservers
  vars:
    ansible_ssh_extra_args: "-o ForwardAgent=yes"
  tasks:
    - name: Clone repository
      git:
        repo: git@github.com:org/app.git
        dest: /opt/app
```

### Host Key Checking

By default, Rustible verifies SSH host keys against `known_hosts`. Control this behavior:

```yaml
# In inventory (per-host)
web1:
  ansible_host: 10.0.0.1
  ansible_ssh_host_key_checking: false   # Disable for this host
```

```bash
# Via environment variable (global)
export RUSTIBLE_HOST_KEY_CHECKING=false
rustible run playbook.yml
```

Manage known hosts programmatically:

```yaml
- name: Add host key
  known_hosts:
    name: server.example.com
    key: "{{ lookup('pipe', 'ssh-keyscan server.example.com') }}"
    state: present
```

## Privilege Escalation (become)

Most system administration tasks require root or elevated privileges. Rustible supports privilege escalation through the `become` system.

### Basic Usage

```yaml
- hosts: webservers
  become: true              # Enable privilege escalation
  become_user: root         # User to become (default: root)
  become_method: sudo       # Method: sudo or su
  tasks:
    - name: Install package
      package:
        name: nginx
        state: present
```

### become Directives

| Directive | Description | Default |
|-----------|-------------|---------|
| `become` | Enable/disable escalation | `false` |
| `become_user` | Target user | `root` |
| `become_method` | Escalation method (`sudo`, `su`) | `sudo` |
| `become_flags` | Additional flags to pass to the method | (none) |

### Scope Levels

Escalation can be set at play, block, or task level:

```yaml
- hosts: webservers
  become: true                # Play level - all tasks escalate

  tasks:
    - name: Read non-privileged file
      command: cat /tmp/readme.txt
      become: false           # Task level - override play setting

    - name: Privileged block
      block:
        - name: Install package
          package:
            name: nginx
        - name: Configure service
          template:
            src: nginx.conf.j2
            dest: /etc/nginx/nginx.conf
      become: true            # Block level
      become_user: root
```

### Sudo Password

When sudo requires a password:

```bash
# Prompt for become password
rustible run playbook.yml --ask-become-pass

# Or set in inventory (encrypted with vault)
ansible_become_pass: !vault |
  $RUSTIBLE_VAULT;1.0;AES256-GCM
  <encrypted-password>
```

### Running as a Different User

Switch to a non-root user for application deployment:

```yaml
- hosts: webservers
  tasks:
    - name: Deploy as app user
      become: true
      become_user: deploy
      copy:
        src: app.tar.gz
        dest: /home/deploy/app.tar.gz
```

## Input Validation and Security Hardening

Rustible includes built-in security measures:

### Command Injection Prevention

All module parameters that are passed to shell commands are validated against shell metacharacters. Package names, paths, and other user-provided values are checked for dangerous characters (`$`, `` ` ``, `|`, `&`, `;`, `<`, `>`, etc.) before execution.

### Path Traversal Protection

File paths are validated to prevent directory traversal attacks. The `validate_path_param` function rejects paths containing sequences like `..` that could escape intended directories.

### Package Name Validation

Package names are validated against a strict pattern (`[a-zA-Z0-9._+-]+`) before being passed to package managers, preventing command injection through crafted package names.

## Secret Backends

Vault password files can be executable scripts, enabling integration with external secret managers:

```bash
#!/bin/bash
# .vault_pass.sh - Fetch password from AWS Secrets Manager
aws secretsmanager get-secret-value \
  --secret-id rustible-vault-password \
  --query SecretString --output text
```

```bash
chmod +x .vault_pass.sh
rustible run playbook.yml --vault-password-file ./.vault_pass.sh
```

This pattern works with any secret backend: HashiCorp Vault, AWS Secrets Manager, Azure Key Vault, GCP Secret Manager, or a simple password manager CLI.

## Best Practices

1. **Never commit unencrypted secrets** to version control. Always encrypt with Vault first.
2. **Add `.vault_pass` to `.gitignore`** to prevent accidental password file commits.
3. **Use vault password files** (or scripts) rather than `--ask-vault-pass` in CI/CD pipelines.
4. **Prefer Ed25519 keys** for SSH -- they are faster, smaller, and more secure than RSA.
5. **Enable host key checking** in production. Only disable it in ephemeral test environments.
6. **Use separate vault IDs** for different environments (dev, staging, production) so that production secrets require a separate password.
7. **Rotate vault passwords** periodically using `vault rekey`.
8. **Limit become scope**. Apply `become: true` only to the tasks that need it, not the entire play.
9. **Audit your playbooks** for hardcoded secrets. Use `grep -r "password\|secret\|token"` to find potential leaks.
10. **Use `no_log: true`** on tasks that handle sensitive data to prevent secrets from appearing in logs:

```yaml
- name: Set database password
  shell: "echo '{{ db_password }}' | /opt/set-password.sh"
  no_log: true
```

## Next Steps

- Learn about [Templating](09-templating.md)
- Review [Best Practices](best-practices.md)
- See the [CLI Reference](cli-reference.md) for all vault command options
