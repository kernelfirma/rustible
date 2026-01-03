---
summary: Using --plan flag for Terraform-like dry-run execution previews showing what tasks would run without making changes.
read_when: Previewing changes before execution, reviewing playbook impact, or integrating plan reviews into change management.
---

# Plan Mode - Dry-Run Execution Planning

## Overview

The `--plan` flag enables dry-run execution planning in Rustible, similar to Terraform's plan feature. When enabled, Rustible will analyze the playbook and display what would be executed without actually making any changes to the target systems.

## Usage

```bash
rustible run playbook.yml --plan
```

### With Other Flags

Plan mode can be combined with other flags:

```bash
# Plan with specific tags
rustible run playbook.yml --plan --tags install

# Plan with extra variables
rustible run playbook.yml --plan -e environment=production

# Plan with inventory limit
rustible run playbook.yml --plan -l webservers

# Plan with verbosity
rustible run playbook.yml --plan -vv
```

## Output Format

The plan output is designed to be clear and actionable, showing:

1. **Execution Plan Header** - Indicates plan mode is active
2. **Play Information** - For each play:
   - Play name and number
   - Target hosts pattern and count
   - Number of tasks
3. **Task Details** - For each task:
   - Task name and number
   - Module being used
   - Per-host action description
   - Conditional expressions (when clauses)
   - Handler notifications
4. **Plan Summary** - Total task and host counts

### Example Output

```
Running in PLAN MODE - showing execution plan only

=========================================================
EXECUTION PLAN
=========================================================

Rustible will perform the following actions:

[Play 1/1] ⚡ Configure Web Servers
  Hosts: webservers (3 hosts)
  Tasks: 5 tasks

  ▸ Task 1/5: Install nginx
    Module: package
      [web1.example.com] will install package: nginx
      [web2.example.com] will install package: nginx
      [web3.example.com] will install package: nginx

  ▸ Task 2/5: Copy nginx configuration
    Module: copy
      [web1.example.com] will copy nginx.conf to /etc/nginx/nginx.conf
      [web2.example.com] will copy nginx.conf to /etc/nginx/nginx.conf
      [web3.example.com] will copy nginx.conf to /etc/nginx/nginx.conf
    Notify: restart nginx

  ▸ Task 3/5: Ensure nginx is running
    Module: service
      [web1.example.com] will started service: nginx
      [web2.example.com] will started service: nginx
      [web3.example.com] will started service: nginx

  ▸ Task 4/5: Create web root directory
    Module: file
      [web1.example.com] will ensure /var/www/html exists as directory
      [web2.example.com] will ensure /var/www/html exists as directory
      [web3.example.com] will ensure /var/www/html exists as directory

  ▸ Task 5/5: Deploy application
    Module: git
      [web1.example.com] will clone/update https://github.com/example/app.git to /var/www/app
      [web2.example.com] will clone/update https://github.com/example/app.git to /var/www/app
      [web3.example.com] will clone/update https://github.com/example/app.git to /var/www/app
    When: deploy_app == true

=========================================================
PLAN SUMMARY
=========================================================

Plan: 5 tasks across 3 hosts

To execute this plan, run the same command without --plan
```

## Module Support

Plan mode provides detailed action descriptions for the following modules:

### Package Management
- `package`, `apt`, `yum`, `dnf`, `pip`
- Shows: install/remove action and package names

### Service Management
- `service`
- Shows: service state changes (started, stopped, restarted)

### File Operations
- `copy` - Shows source and destination paths
- `file` - Shows path and state (file, directory, absent)
- `template` - Shows template source and destination

### System Commands
- `command`, `shell`
- Shows: exact command to be executed (with variable substitution)

### User/Group Management
- `user` - Shows create/update/remove actions
- `group` - Shows create/update/remove actions

### Version Control
- `git` - Shows repository URL and destination path

### Text File Editing
- `lineinfile` - Shows file path being modified
- `blockinfile` - Shows file path being modified

### Other Modules
- `debug` - Shows message or variable to be displayed
- `set_fact` - Indicates fact setting
- Unknown modules - Shows generic module execution message

## Features

### Variable Substitution

Plan mode performs template variable substitution in action descriptions:

```yaml
vars:
  package_name: nginx
  dest_path: /etc/nginx

tasks:
  - name: Install package
    package:
      name: "{{ package_name }}"
```

Output:
```
will install package: nginx
```

### Conditional Display

Tasks with `when` conditions show the condition in the plan:

```yaml
- name: Install on Debian
  apt:
    name: nginx
  when: ansible_os_family == "Debian"
```

Output:
```
When: ansible_os_family == "Debian"
```

### Handler Notifications

Tasks that notify handlers display which handlers will be triggered:

```yaml
- name: Update config
  copy:
    src: app.conf
    dest: /etc/app.conf
  notify:
    - restart app
    - reload config
```

Output:
```
Notify: restart app, reload config
```

### Tag Filtering

Plan mode respects `--tags` and `--skip-tags` filters:

```bash
rustible run playbook.yml --plan --tags install
```

Only shows tasks tagged with "install".

## Comparison with Check Mode

While both `--plan` and `--check` are dry-run modes, they serve different purposes:

| Feature | `--plan` | `--check` |
|---------|----------|-----------|
| **Purpose** | Show execution plan | Test playbook without changes |
| **Execution** | No tasks executed | Tasks run in simulation mode |
| **Output** | Formatted plan view | Standard task output |
| **Performance** | Fast (no connections) | Slower (connects to hosts) |
| **Use Case** | Planning/review | Testing/validation |
| **Shows** | What will happen | What would happen if run |

### When to Use Each

**Use `--plan` when:**
- Reviewing changes before execution
- Understanding playbook impact
- Documentation and change management
- Quick overview of actions

**Use `--check` when:**
- Testing playbook validity
- Checking for errors
- Validating conditionals with actual facts
- Testing against real systems

## Best Practices

### 1. Review Before Deployment

Always run plan mode before executing playbooks in production:

```bash
# Review the plan
rustible run deploy.yml --plan -e environment=production

# If plan looks good, execute
rustible run deploy.yml -e environment=production
```

### 2. Document Changes

Save plan output for change management:

```bash
rustible run playbook.yml --plan > deployment-plan.txt
```

### 3. Verify Tag Filters

Test tag filters with plan mode to ensure correct task selection:

```bash
rustible run playbook.yml --plan --tags database
```

### 4. Check Variable Substitution

Verify that variables are substituted correctly:

```bash
rustible run playbook.yml --plan -e @vars/production.yml
```

### 5. Combine with Limit

Preview changes for specific hosts:

```bash
rustible run playbook.yml --plan -l webserver1
```

## Exit Codes

Plan mode uses the same exit codes as normal execution:

- `0` - Success (plan generated successfully)
- `1` - Error (playbook parse error, invalid arguments, etc.)

Note: Plan mode always returns success if the playbook is valid, even if tasks would fail during actual execution.

## Limitations

1. **No Remote Connections** - Plan mode doesn't connect to hosts, so it can't:
   - Gather facts
   - Check current state
   - Validate conditionals that depend on host facts

2. **Template Approximation** - Variable substitution is best-effort:
   - Only variables in the playbook scope are resolved
   - Complex Jinja2 filters may not be fully evaluated

3. **No Handler Execution** - Handler execution isn't simulated

4. **No Include Resolution** - Dynamic includes (`include_tasks`) show as-is

## Examples

### Basic Plan

```bash
rustible run site.yml --plan
```

### Plan for Specific Environment

```bash
rustible run deploy.yml --plan \
  -e @vars/production.yml \
  -l production-servers
```

### Plan with Tag Selection

```bash
rustible run maintenance.yml --plan \
  --tags backup,cleanup \
  --skip-tags dangerous
```

### Verbose Plan

```bash
rustible run playbook.yml --plan -vv
```

## Integration with CI/CD

Plan mode integrates well with CI/CD pipelines:

```yaml
# GitLab CI example
plan:
  stage: plan
  script:
    - rustible run deploy.yml --plan -e environment=${CI_ENVIRONMENT_NAME}
  artifacts:
    paths:
      - deployment-plan.txt

deploy:
  stage: deploy
  script:
    - rustible run deploy.yml -e environment=${CI_ENVIRONMENT_NAME}
  when: manual
  dependencies:
    - plan
```

## See Also

- [Check Mode Documentation](check_mode.md)
- [Playbook Syntax](playbook_syntax.md)
- [Variables and Templating](variables.md)
- [Tags](tags.md)
