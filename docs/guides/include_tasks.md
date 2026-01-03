---
summary: Using include_tasks, import_tasks, and include_vars for modular playbooks with proper variable scoping and task organization.
read_when: Breaking playbooks into reusable components, managing variable files, or understanding include vs import semantics.
---

# Task and Variable Inclusion

Rustible supports Ansible-compatible task and variable inclusion with three main directives:

- `include_tasks`: Dynamically include tasks with separate variable scope
- `import_tasks`: Statically import tasks at parse time (merged scope)
- `include_vars`: Load variables from YAML files during execution

## include_tasks

`include_tasks` loads tasks dynamically during playbook execution. Each inclusion creates a **separate variable scope**, meaning variables passed to the included tasks don't affect the parent play.

### Basic Usage

```yaml
---
- name: Example playbook with include_tasks
  hosts: all
  tasks:
    - name: Include common setup tasks
      include_tasks: tasks/setup.yml

    - name: Include with variables
      include_tasks:
        file: tasks/deploy.yml
        vars:
          app_name: "my_app"
          app_version: "1.2.3"
```

### tasks/setup.yml

```yaml
---
- name: Update package cache
  apt:
    update_cache: yes

- name: Install required packages
  apt:
    name:
      - git
      - curl
      - build-essential
    state: present
```

### tasks/deploy.yml

```yaml
---
- name: Deploy application
  debug:
    msg: "Deploying {{ app_name }} version {{ app_version }}"

- name: Create app directory
  file:
    path: "/opt/{{ app_name }}"
    state: directory
```

### Variable Scope

Variables passed via `include_tasks` are available **only** to the included tasks:

```yaml
- name: Include with scoped vars
  include_tasks:
    file: tasks/database.yml
    vars:
      db_password: "secret123"  # Only available in database.yml

- name: This task won't see db_password
  debug:
    msg: "{{ db_password }}"  # ERROR: undefined variable
```

## import_tasks

`import_tasks` loads tasks statically at playbook parse time. Variables are **merged into the parent scope**.

### Basic Usage

```yaml
---
- name: Example playbook with import_tasks
  hosts: all
  tasks:
    - name: Import pre-tasks
      import_tasks: tasks/pre_setup.yml

    - name: Import with variables
      import_tasks:
        file: tasks/config.yml
        vars:
          config_mode: "production"
```

### Difference from include_tasks

```yaml
# import_tasks: Variables merge into parent scope
- name: Import with vars
  import_tasks:
    file: tasks/set_vars.yml
    vars:
      my_var: "shared_value"

- name: Can access imported var
  debug:
    msg: "{{ my_var }}"  # Works! my_var is in parent scope
```

### tasks/set_vars.yml

```yaml
---
- name: Set configuration
  set_fact:
    app_config:
      debug: true
      port: 8080
```

### When to Use import_tasks vs include_tasks

| Feature | import_tasks | include_tasks |
|---------|-------------|---------------|
| Execution | Parse time (static) | Runtime (dynamic) |
| Variable scope | Merged with parent | Separate scope |
| Conditional inclusion | Limited | Full support with `when` |
| Tags | Always applied | Applied based on `apply` setting |
| Performance | Faster (pre-loaded) | Slower (loaded on demand) |

## include_vars

`include_vars` loads variables from YAML files during playbook execution at the `IncludeVars` precedence level (higher than PlayVars).

### Basic Usage

```yaml
---
- name: Load variables from file
  hosts: all
  tasks:
    - name: Load common variables
      include_vars: vars/common.yml

    - name: Load environment-specific vars
      include_vars: "vars/{{ environment }}.yml"

    - name: Use loaded variables
      debug:
        msg: "App {{ app_name }} running on port {{ app_port }}"
```

### vars/common.yml

```yaml
---
app_name: "my_application"
app_user: "appuser"
app_group: "appgroup"
```

### vars/production.yml

```yaml
---
app_port: 8080
app_debug: false
app_workers: 4
database:
  host: "prod-db.example.com"
  port: 5432
```

### Variable Precedence

`include_vars` loads at `IncludeVars` precedence (16), which overrides:

- PlayVars (10)
- PlayVarsFiles (12)
- RoleVars (13)

But is overridden by:

- SetFacts (17)
- RoleParams (18)
- IncludeParams (19)
- ExtraVars (20)

```yaml
- name: Demonstrate precedence
  hosts: all
  vars:
    my_var: "play_value"  # Precedence: 10
  tasks:
    - name: Load vars file
      include_vars: my_vars.yml  # Precedence: 16 (wins!)

    - debug:
        msg: "{{ my_var }}"  # Shows value from my_vars.yml
```

## Advanced Examples

### Conditional Inclusion

```yaml
---
- name: Conditional include
  hosts: all
  tasks:
    - name: Include tasks based on OS
      include_tasks: "tasks/{{ ansible_os_family }}.yml"
      when: ansible_os_family in ['Debian', 'RedHat']

    - name: Include only on production
      include_tasks:
        file: tasks/production_setup.yml
      when: environment == "production"
```

### Nested Inclusions

```yaml
# main.yml
---
- name: Main playbook
  hosts: all
  tasks:
    - include_tasks: tasks/level1.yml

# tasks/level1.yml
---
- name: Level 1 task
  debug:
    msg: "At level 1"

- include_tasks: level2.yml

# tasks/level2.yml
---
- name: Level 2 task
  debug:
    msg: "At level 2"
```

### Loop with include_tasks

```yaml
---
- name: Include tasks in a loop
  hosts: all
  tasks:
    - name: Deploy multiple applications
      include_tasks: tasks/deploy_app.yml
      vars:
        app: "{{ item }}"
      loop:
        - web_app
        - api_server
        - background_worker
```

### Complex Variable Files

```yaml
---
- name: Load structured configuration
  hosts: all
  tasks:
    - name: Load database config
      include_vars: config/database.yml

    - name: Load service config
      include_vars: config/services.yml

    - name: Deploy with all configs
      template:
        src: app_config.j2
        dest: /etc/app/config.yml
```

## Implementation Details

### TaskIncluder API

```rust
use rustible::include::{TaskIncluder, IncludeTasksSpec, ImportTasksSpec};
use rustible::vars::{VarStore, VarPrecedence};

// Create an includer with base path
let includer = TaskIncluder::new("/path/to/playbook");

// Load include_tasks (separate scope)
let spec = IncludeTasksSpec::new("tasks/setup.yml")
    .with_var("var1", serde_json::json!("value1"))
    .with_var("var2", serde_json::json!(123));

let parent_vars = VarStore::new();
let (tasks, scope) = includer
    .load_include_tasks(&spec, &parent_vars)
    .await?;

// Load import_tasks (merged scope)
let spec = ImportTasksSpec::new("tasks/config.yml")
    .with_var("config_key", serde_json::json!("value"));

let mut parent_vars = VarStore::new();
let tasks = includer
    .load_import_tasks(&spec, &mut parent_vars)
    .await?;

// Load include_vars
let mut var_store = VarStore::new();
includer
    .load_vars_from_file("vars/app.yml", &mut var_store)
    .await?;
```

### Error Handling

All include operations return `Result<T>` and handle:

- File not found errors
- YAML parsing errors
- Invalid variable types
- Path resolution issues

```rust
use rustible::error::Error;

match includer.load_include_tasks(&spec, &vars).await {
    Ok((tasks, scope)) => {
        // Process tasks
    }
    Err(Error::FileNotFound(path)) => {
        eprintln!("Task file not found: {:?}", path);
    }
    Err(Error::PlaybookParse { message, .. }) => {
        eprintln!("Failed to parse tasks: {}", message);
    }
    Err(e) => {
        eprintln!("Unexpected error: {}", e);
    }
}
```

## Testing

See `tests/include_vars_tests.rs` for comprehensive test examples covering:

- Basic include_tasks with separate scope
- import_tasks with merged scope
- include_vars with precedence
- Nested inclusions
- Error handling
- Complex variable structures
