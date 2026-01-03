---
summary: Solutions for common issues including connection problems, playbook errors, module failures, variable issues, and vault problems.
read_when: You encounter errors or unexpected behavior and need to diagnose and fix issues.
---

# Troubleshooting Guide

This guide covers common issues you may encounter when using Rustible and how to resolve them.

## Table of Contents

- [Connection Issues](#connection-issues)
- [Playbook Errors](#playbook-errors)
- [Module Issues](#module-issues)
- [Variable Problems](#variable-problems)
- [Performance Issues](#performance-issues)
- [Vault and Encryption](#vault-and-encryption)
- [Privilege Escalation](#privilege-escalation)
- [Debugging Techniques](#debugging-techniques)

---

## Connection Issues

### SSH Connection Failed

**Symptom:**
```
ERROR: Connection failed to 'host1': Connection refused
```

**Possible Causes and Solutions:**

1. **SSH service not running on target**
   ```bash
   # On target host
   sudo systemctl status sshd
   sudo systemctl start sshd
   ```

2. **Wrong port**
   ```yaml
   # In inventory
   host1:
     ansible_host: 192.168.1.10
     ansible_port: 2222  # Custom SSH port
   ```

3. **Firewall blocking connection**
   ```bash
   # On target host
   sudo firewall-cmd --add-service=ssh --permanent
   sudo firewall-cmd --reload
   ```

4. **SSH key not configured**
   ```bash
   # Copy your key to the target
   ssh-copy-id user@host1

   # Or specify key in inventory
   ansible_ssh_private_key_file: ~/.ssh/custom_key
   ```

### Authentication Failed

**Symptom:**
```
ERROR: Authentication failed: Permission denied (publickey,password)
```

**Solutions:**

1. **Check SSH key permissions**
   ```bash
   chmod 600 ~/.ssh/id_rsa
   chmod 700 ~/.ssh
   ```

2. **Verify correct user**
   ```yaml
   # In inventory
   host1:
     ansible_user: correct_username
   ```

3. **Test SSH manually**
   ```bash
   ssh -v user@host1
   ```

4. **Check authorized_keys on target**
   ```bash
   # On target host
   cat ~/.ssh/authorized_keys
   ```

### Host Key Verification Failed

**Symptom:**
```
ERROR: Host key verification failed
```

**Solutions:**

1. **Accept the host key**
   ```bash
   ssh user@host1  # Accept the key when prompted
   ```

2. **Clear old host key (if host was reinstalled)**
   ```bash
   ssh-keygen -R host1
   ```

3. **Disable host key checking (development only)**
   ```yaml
   # In rustible.toml
   [ssh]
   host_key_checking = false
   ```

### Connection Timeout

**Symptom:**
```
ERROR: Connection timed out to 'host1'
```

**Solutions:**

1. **Increase timeout**
   ```bash
   rustible run playbook.yml --timeout 60
   ```

2. **Check network connectivity**
   ```bash
   ping host1
   telnet host1 22
   ```

3. **Verify DNS resolution**
   ```bash
   nslookup host1
   ```

---

## Playbook Errors

### YAML Syntax Error

**Symptom:**
```
ERROR: Failed to parse playbook: YAML error at line 15, column 3
```

**Common Causes:**

1. **Incorrect indentation**
   ```yaml
   # Wrong
   tasks:
   - name: Task 1
       debug:
         msg: "Hello"

   # Correct
   tasks:
     - name: Task 1
       debug:
         msg: "Hello"
   ```

2. **Missing quotes around special characters**
   ```yaml
   # Wrong - colon causes issues
   msg: Error: something failed

   # Correct
   msg: "Error: something failed"
   ```

3. **Tab characters (use spaces only)**
   ```bash
   # Check for tabs
   cat -A playbook.yml | grep '\^I'
   ```

**Validation:**
```bash
# Validate syntax before running
rustible validate playbook.yml
```

### Module Not Found

**Symptom:**
```
ERROR: Module 'my_custom_module' not found
```

**Solutions:**

1. **Check module name spelling**
   ```yaml
   # Common mistakes
   - name: Wrong
     commnad: echo hello  # Typo

   # Correct
   - name: Right
     command: echo hello
   ```

2. **Use FQCN for Ansible modules**
   ```yaml
   - name: Use full name
     ansible.builtin.command: echo hello
   ```

3. **Check available modules**
   ```bash
   rustible --help  # Lists available modules
   ```

### Task Failed

**Symptom:**
```
TASK [Install package] *************************
host1: FAILED! => {"msg": "Package not found: nonexistent-pkg"}
```

**Solutions:**

1. **Run with verbose mode**
   ```bash
   rustible run playbook.yml -vvv
   ```

2. **Use check mode first**
   ```bash
   rustible run playbook.yml --check
   ```

3. **Add error handling**
   ```yaml
   - name: Try to install
     package:
       name: might-not-exist
     ignore_errors: true
     register: result

   - name: Handle failure
     debug:
       msg: "Package not available"
     when: result.failed
   ```

---

## Module Issues

### Command Module Failures

**Symptom:**
```
FAILED! => {"rc": 127, "msg": "command not found: my_script.sh"}
```

**Solutions:**

1. **Use absolute path**
   ```yaml
   - command: /usr/local/bin/my_script.sh
   ```

2. **Use shell module for PATH resolution**
   ```yaml
   - shell: my_script.sh
   ```

3. **Set PATH explicitly**
   ```yaml
   - command: my_script.sh
     environment:
       PATH: "/usr/local/bin:{{ ansible_env.PATH }}"
   ```

### File Module Permission Denied

**Symptom:**
```
FAILED! => {"msg": "Permission denied: /etc/important.conf"}
```

**Solutions:**

1. **Add become**
   ```yaml
   - name: Edit system file
     file:
       path: /etc/important.conf
       mode: '0644'
     become: true
   ```

2. **Verify become user has permissions**
   ```yaml
   - name: As specific user
     file:
       path: /opt/app/config
       mode: '0644'
     become: true
     become_user: app_user
   ```

### Template Rendering Error

**Symptom:**
```
ERROR: Template rendering failed: undefined variable 'missing_var'
```

**Solutions:**

1. **Provide default value**
   ```jinja2
   server_name = {{ server_name | default('localhost') }}
   ```

2. **Check if variable exists**
   ```jinja2
   {% if database is defined %}
   db_host = {{ database.host }}
   {% endif %}
   ```

3. **Debug variable values**
   ```yaml
   - debug:
       var: database
   ```

---

## Variable Problems

### Undefined Variable

**Symptom:**
```
ERROR: Variable 'app_version' is undefined
```

**Solutions:**

1. **Define in play vars**
   ```yaml
   - hosts: all
     vars:
       app_version: "1.0.0"
   ```

2. **Define in inventory**
   ```yaml
   all:
     vars:
       app_version: "1.0.0"
   ```

3. **Pass as extra vars**
   ```bash
   rustible run playbook.yml -e "app_version=1.0.0"
   ```

4. **Use default filter**
   ```yaml
   version: "{{ app_version | default('latest') }}"
   ```

### Variable Precedence Confusion

**Symptom:** Variable has unexpected value

**Debug Steps:**

1. **Print all variable sources**
   ```yaml
   - name: Debug variable sources
     debug:
       msg: |
         play_var: {{ play_var | default('not set') }}
         hostvars: {{ hostvars[inventory_hostname].play_var | default('not set') }}
   ```

2. **Remember precedence order** (lowest to highest):
   - Role defaults
   - Inventory group_vars
   - Inventory host_vars
   - Play vars
   - Role vars
   - Task vars
   - Extra vars (always wins)

3. **Extra vars always win**
   ```bash
   # This will always override any other setting
   rustible run playbook.yml -e "my_var=override"
   ```

### Boolean Conversion Issues

**Symptom:** Condition not working as expected

**Solutions:**

1. **Use explicit boolean filter**
   ```yaml
   when: my_var | bool
   ```

2. **Be careful with string "false"**
   ```yaml
   # String "false" is truthy!
   when: my_var == true  # Use explicit comparison
   ```

3. **YAML boolean values**
   ```yaml
   # These are boolean true
   enabled: true
   enabled: yes
   enabled: on

   # These are boolean false
   enabled: false
   enabled: no
   enabled: off
   ```

---

## Performance Issues

### Slow Execution

**Symptoms:**
- Playbook takes longer than expected
- High connection overhead

**Solutions:**

1. **Increase forks**
   ```bash
   rustible run playbook.yml -f 20
   ```

2. **Use free strategy**
   ```yaml
   - hosts: all
     strategy: free
   ```

3. **Disable fact gathering when not needed**
   ```yaml
   - hosts: all
     gather_facts: false
   ```

4. **Use tags to run specific tasks**
   ```bash
   rustible run playbook.yml --tags deploy
   ```

5. **Batch file operations**
   ```yaml
   # Instead of multiple copies
   - name: Copy files as archive
     unarchive:
       src: files.tar.gz
       dest: /opt/app/
   ```

### Memory Usage High

**Solutions:**

1. **Reduce forks for large inventories**
   ```bash
   rustible run playbook.yml -f 10
   ```

2. **Split large playbooks**
   ```bash
   # Run in stages
   rustible run stage1.yml -i inventory.yml
   rustible run stage2.yml -i inventory.yml
   ```

3. **Use limits for testing**
   ```bash
   rustible run playbook.yml --limit web1
   ```

---

## Vault and Encryption

### Wrong Vault Password

**Symptom:**
```
ERROR: Decryption failed: Invalid vault password
```

**Solutions:**

1. **Verify password file**
   ```bash
   cat ~/.vault_pass  # Check contents
   ```

2. **Check file permissions**
   ```bash
   chmod 600 ~/.vault_pass
   ```

3. **Try re-encrypting**
   ```bash
   rustible vault decrypt secrets.yml
   rustible vault encrypt secrets.yml  # With new password
   ```

### Vault File Corrupted

**Symptom:**
```
ERROR: Failed to decrypt vault file: Invalid format
```

**Solutions:**

1. **Check file format**
   ```bash
   head -1 secrets.yml
   # Should start with $RUSTIBLE_VAULT
   ```

2. **Restore from backup**
   ```bash
   cp secrets.yml.backup secrets.yml
   ```

### Encrypting Specific Variables

**Problem:** Only want to encrypt sensitive values, not entire file

**Solution:**
```bash
# Encrypt just the password
rustible vault encrypt-string "my_password" -p db_password

# Use inline encrypted variable in YAML
db_password: !vault |
  $RUSTIBLE_VAULT;1.0;AES256-GCM
  [encrypted content]
```

---

## Privilege Escalation

### Become Password Required

**Symptom:**
```
ERROR: Become password required but not provided
```

**Solutions:**

1. **Provide password interactively**
   ```bash
   rustible run playbook.yml -K
   ```

2. **Configure passwordless sudo**
   ```bash
   # On target host, add to /etc/sudoers.d/deploy
   deploy ALL=(ALL) NOPASSWD:ALL
   ```

3. **Use SSH agent forwarding with NOPASSWD**

### Become User Not Found

**Symptom:**
```
ERROR: become user 'app_user' does not exist
```

**Solutions:**

1. **Create user first**
   ```yaml
   - name: Create app user
     user:
       name: app_user
       state: present
     become: true

   - name: Run as app user
     command: whoami
     become: true
     become_user: app_user
   ```

2. **Verify user exists**
   ```bash
   ssh host1 "id app_user"
   ```

---

## Debugging Techniques

### Verbose Output

```bash
# Increasing verbosity levels
rustible run playbook.yml -v      # Basic
rustible run playbook.yml -vv     # More detail
rustible run playbook.yml -vvv    # Connection info
rustible run playbook.yml -vvvv   # Maximum detail
```

### Debug Module

```yaml
tasks:
  - name: Print variable
    debug:
      var: my_variable

  - name: Print message
    debug:
      msg: "Value is {{ my_variable }}"

  - name: Print all variables
    debug:
      var: hostvars[inventory_hostname]
```

### Check Mode

```bash
# Dry run - show what would change
rustible run playbook.yml --check

# With diff output
rustible run playbook.yml --check --diff
```

### Plan Mode

```bash
# Show execution plan
rustible run playbook.yml --plan
```

### Step Mode

```bash
# Step through tasks one at a time
rustible run playbook.yml --step
```

### Start at Specific Task

```bash
# Skip to specific task
rustible run playbook.yml --start-at-task "Deploy application"
```

### Limit Execution

```bash
# Test on single host
rustible run playbook.yml --limit host1

# Test on group
rustible run playbook.yml --limit webservers

# Exclude hosts
rustible run playbook.yml --limit 'all:!problematic_host'
```

### Assert Module

```yaml
tasks:
  - name: Verify prerequisites
    assert:
      that:
        - ansible_os_family == "Debian"
        - app_version is defined
        - app_version is version('1.0', '>=')
      fail_msg: "Prerequisites not met"
      success_msg: "All checks passed"
```

---

## Getting More Help

If you can't resolve your issue:

1. **Check the documentation**: Review relevant sections of this guide
2. **Search existing issues**: [GitHub Issues](https://github.com/rustible/rustible/issues)
3. **Open a new issue**: Include:
   - Rustible version (`rustible --version`)
   - OS and version
   - Minimal playbook to reproduce
   - Full error output with `-vvv`
   - Expected vs actual behavior
