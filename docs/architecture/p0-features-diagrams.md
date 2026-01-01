# P0 Features Architecture Diagrams

**Related:** [P0 Features Design Document](./p0-features-design.md)

## System Context Diagram (C4 Level 1)

```
┌────────────────────────────────────────────────────────────────────────┐
│                          Rustible System                                │
│                                                                         │
│  ┌─────────────┐         ┌──────────────┐        ┌─────────────┐      │
│  │             │         │              │        │             │      │
│  │  CLI User   │────────▶│   Rustible   │───────▶│  Remote     │      │
│  │             │  cmds   │   Executor   │  SSH   │  Hosts      │      │
│  │             │         │              │        │             │      │
│  └─────────────┘         └──────┬───────┘        └─────────────┘      │
│                                 │                                      │
│                                 │ uses                                 │
│                                 ▼                                      │
│                          ┌──────────────┐                              │
│                          │   Ansible    │                              │
│                          │   Python     │                              │
│                          │   Modules    │                              │
│                          └──────────────┘                              │
└────────────────────────────────────────────────────────────────────────┘
```

## Container Diagram (C4 Level 2)

```
┌────────────────────────────────────────────────────────────────────────┐
│                         Rustible Application                            │
│                                                                         │
│  ┌─────────────────────────────────────────────────────────┐           │
│  │                      CLI Layer                           │           │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────────┐          │           │
│  │  │ RunArgs  │─▶│ Playbook │─▶│ Connection   │          │           │
│  │  │ Parser   │  │ Loader   │  │ Pool Manager │          │           │
│  │  └──────────┘  └──────────┘  └──────────────┘          │           │
│  └──────────────────────┬──────────────────────────────────┘           │
│                         │ creates ExecutionContext                     │
│                         ▼                                              │
│  ┌─────────────────────────────────────────────────────────┐           │
│  │                   Executor Layer                         │           │
│  │  ┌────────────┐  ┌──────────────┐  ┌────────────────┐  │           │
│  │  │   Task     │─▶│   Module     │─▶│  Module        │  │           │
│  │  │ Execution  │  │   Registry   │  │  Executor      │  │           │
│  │  └────────────┘  └──────────────┘  └────────────────┘  │           │
│  │         │                                   │            │           │
│  │         │ resolves become                  │ creates    │           │
│  │         ▼                                   ▼            │           │
│  │  ┌────────────┐                    ┌────────────────┐  │           │
│  │  │  Become    │                    │ ModuleContext  │  │           │
│  │  │  Config    │                    │ (with become)  │  │           │
│  │  └────────────┘                    └────────────────┘  │           │
│  └──────────────────────┬──────────────────────────────────┘           │
│                         │ uses                                         │
│                         ▼                                              │
│  ┌─────────────────────────────────────────────────────────┐           │
│  │                 Connection Layer                         │           │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │           │
│  │  │ Execute      │─▶│  Connection  │─▶│ SSH/Local/   │  │           │
│  │  │ Options      │  │  Trait       │  │ Docker       │  │           │
│  │  │ (escalation) │  │              │  │ (sudo wrap)  │  │           │
│  │  └──────────────┘  └──────────────┘  └──────────────┘  │           │
│  └─────────────────────────────────────────────────────────┘           │
└────────────────────────────────────────────────────────────────────────┘
```

## Component Diagram: Become Flow (C4 Level 3)

```
┌────────────────────────────────────────────────────────────────────────┐
│                        Become Resolution Flow                           │
└────────────────────────────────────────────────────────────────────────┘

┌─────────────┐
│   RunArgs   │  CLI arguments
│             │
│ become: bool│───┐
│ user: String│   │
│ method: Str │   │
└─────────────┘   │
                  │
                  │ precedence
                  │ resolution
┌─────────────┐   │              ┌──────────────────┐
│    Play     │   │              │  BecomeConfig    │
│             │   │              │                  │
│ become: Opt │───┼─────────────▶│  ::resolve()     │
│ user: Option│   │              │                  │
└─────────────┘   │              │  ┌────────────┐  │
                  │              │  │Precedence: │  │
┌─────────────┐   │              │  │1. Task     │  │
│    Task     │   │              │  │2. Play     │  │
│             │   │              │  │3. CLI      │  │
│ become: bool│───┘              │  │4. Default  │  │
│ user: Option│                  │  └────────────┘  │
└─────────────┘                  └─────────┬────────┘
                                           │
                                           │ creates
                                           ▼
                          ┌────────────────────────────┐
                          │    ExecutionContext        │
                          │                            │
                          │  become: BecomeConfig {    │
                          │    enabled: true,          │
                          │    user: "admin",          │
                          │    method: "sudo"          │
                          │  }                         │
                          └────────┬───────────────────┘
                                   │
                    ┌──────────────┴──────────────┐
                    │                             │
                    ▼                             ▼
        ┌───────────────────┐         ┌─────────────────────┐
        │  ModuleContext    │         │  ExecuteOptions     │
        │                   │         │                     │
        │  become: true     │         │  escalation:        │
        │  become_user:     │         │    Some("admin")    │
        │    Some("admin")  │         │                     │
        │  become_method:   │         └──────────┬──────────┘
        │    Some("sudo")   │                    │
        └─────────┬─────────┘                    │
                  │                              │
                  ▼                              ▼
        ┌───────────────────┐         ┌─────────────────────┐
        │  Native Module    │         │  Connection::exec   │
        │  Execution        │         │                     │
        │  (uses context)   │         │  Wraps command:     │
        └───────────────────┘         │  "sudo -u admin $CMD│
                                      └─────────────────────┘
```

## Component Diagram: Module Execution Decision Tree

```
┌────────────────────────────────────────────────────────────────────────┐
│                     Module Execution Router                             │
└────────────────────────────────────────────────────────────────────────┘

                         Task.module = "command"
                                 │
                                 │
                                 ▼
                   ┌─────────────────────────┐
                   │ Task::execute_module()  │
                   └────────────┬────────────┘
                                │
                                │ dispatch by module name
                                │
                ┌───────────────┼───────────────┐
                │               │               │
                ▼               ▼               ▼
        ┌───────────┐   ┌──────────┐   ┌──────────────┐
        │  debug    │   │ set_fact │   │    copy      │
        │  (native) │   │ (native) │   │   (native)   │
        └─────┬─────┘   └────┬─────┘   └──────┬───────┘
              │              │                 │
              │              │                 │
              └──────────────┼─────────────────┘
                             │
                             │ All native modules
                             │ execute directly
                             ▼
                    ┌────────────────┐
                    │ Return         │
                    │ TaskResult     │
                    │ (real result)  │
                    └────────────────┘


                         Task.module = "service"
                                 │
                                 │
                                 ▼
                   ┌─────────────────────────┐
                   │ Task::execute_module()  │
                   └────────────┬────────────┘
                                │
                                │ fallthrough to default
                                ▼
                   ┌─────────────────────────┐
                   │ PythonModuleExecutor    │
                   │  .find_module()         │
                   └────────────┬────────────┘
                                │
                      ┌─────────┴────────┐
                      │                  │
                  Found?              Not found?
                      │                  │
                      ▼                  ▼
            ┌─────────────────┐   ┌───────────────┐
            │ Connection      │   │ ERROR:        │
            │ available?      │   │ ModuleNotFound│
            └────┬────────────┘   │               │
                 │                │ "Module 'svc' │
          ┌──────┴──────┐         │  not found."  │
          │             │         │               │
        YES            NO          │ Install Ans'  │
          │             │         │ or set path"  │
          ▼             ▼         └───────────────┘
    ┌──────────┐  ┌──────────┐
    │ Execute  │  │ ERROR:   │
    │ via      │  │ ConnReq'd│
    │ Python   │  │          │
    └────┬─────┘  └──────────┘
         │
         ▼
    ┌──────────────┐
    │ Return       │
    │ TaskResult   │
    │ (real result)│
    └──────────────┘


ELIMINATED PATHS (removed in this design):
  ❌ Simulate execution (return changed=true without work)
  ❌ "Would execute..." debug messages
  ❌ Stubbed module methods (execute_package, execute_service, etc.)
```

## Sequence Diagram: End-to-End Become Execution

```
┌──────┐   ┌─────┐   ┌──────┐   ┌──────┐   ┌────────┐   ┌──────────┐
│ User │   │ CLI │   │ Task │   │Module│   │ Become │   │Connection│
└───┬──┘   └──┬──┘   └───┬──┘   └───┬──┘   └───┬────┘   └────┬─────┘
    │         │          │          │          │             │
    │ rustible run       │          │          │             │
    │ --become           │          │          │             │
    │ --become-user app  │          │          │             │
    ├────────▶│          │          │          │             │
    │         │          │          │          │             │
    │         │ Parse args│         │          │             │
    │         │ become=true         │          │             │
    │         │ user="app"│         │          │             │
    │         │          │          │          │             │
    │         │ Load playbook        │          │             │
    │         │ play.become=true    │          │             │
    │         │ play.user="postgres"│          │             │
    │         │          │          │          │             │
    │         │ Execute task         │          │             │
    │         │ task.become=true    │          │             │
    │         │ task.user="admin"   │          │             │
    │         ├──────────▶│          │          │             │
    │         │          │          │          │             │
    │         │          │ Resolve become      │             │
    │         │          │ config   │          │             │
    │         │          ├──────────┼─────────▶│             │
    │         │          │          │          │             │
    │         │          │          │ resolve( │             │
    │         │          │          │  task: true, "admin",  │
    │         │          │          │  play: true, "postgres"│
    │         │          │          │  cli:  true, "app")    │
    │         │          │          │          │             │
    │         │          │          │◀─────────┤             │
    │         │          │          │ BecomeConfig {         │
    │         │          │          │   enabled: true,       │
    │         │          │          │   user: "admin"  ←─ Task wins!
    │         │          │          │ }        │             │
    │         │          │          │          │             │
    │         │          │ Create ExecutionContext           │
    │         │          │ with become config   │            │
    │         │          │          │          │             │
    │         │          │ Execute module        │            │
    │         │          ├──────────▶│          │             │
    │         │          │          │          │             │
    │         │          │          │ Create ModuleContext   │
    │         │          │          │ become: true           │
    │         │          │          │ become_user: "admin"   │
    │         │          │          │          │             │
    │         │          │          │ Execute command        │
    │         │          │          ├────────────────────────▶│
    │         │          │          │          │ Build       │
    │         │          │          │          │ ExecuteOpts │
    │         │          │          │          │ escalation: │
    │         │          │          │          │  Some("admin")
    │         │          │          │          │             │
    │         │          │          │          │ SSH execute │
    │         │          │          │          │ sudo -u admin
    │         │          │          │          │  <command>  │
    │         │          │          │          │             │
    │         │          │          │◀────────────────────────┤
    │         │          │          │ CommandResult {        │
    │         │          │          │   exit_code: 0,        │
    │         │          │          │   stdout: "...",       │
    │         │          │          │   success: true        │
    │         │          │          │ }        │             │
    │         │          │          │          │             │
    │         │          │◀─────────┤          │             │
    │         │          │ TaskResult {         │             │
    │         │          │   changed: true,     │             │
    │         │          │   result: {...}      │             │
    │         │          │ }        │          │             │
    │         │          │          │          │             │
    │         │◀─────────┤          │          │             │
    │         │ Success  │          │          │             │
    │         │          │          │          │             │
    │◀────────┤          │          │          │             │
    │ Exit 0  │          │          │          │             │
    │         │          │          │          │             │
```

## Data Structure Diagram: BecomeConfig Type System

```
┌─────────────────────────────────────────────────────────────────┐
│                       Type Hierarchy                             │
└─────────────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────────┐
│ BecomeConfig (NEW)                                         │
│ ──────────────────────────────────────────────────────────│
│ Fields:                                                    │
│   + enabled: bool                                          │
│   + method: String                  // "sudo", "su", etc. │
│   + user: String                    // "root", "app", etc.│
│   + password: Option<String>        // For future use     │
│   + flags: Option<Vec<String>>      // Extra args         │
│                                                            │
│ Methods:                                                   │
│   + resolve(task, play, cli) -> BecomeConfig              │
│   + to_execute_options() -> Option<String>                │
│   + is_enabled() -> bool                                  │
└────────────────┬───────────────────────────────────────────┘
                 │
                 │ used by
                 │
    ┌────────────┼───────────────────────┐
    │            │                       │
    ▼            ▼                       ▼
┌───────────────────┐    ┌───────────────────┐    ┌───────────────┐
│ExecutionContext   │    │  ModuleContext    │    │ExecuteOptions │
│─────────────────  │    │─────────────────  │    │───────────────│
│ become:           │    │ r#become: bool    │    │ escalation:   │
│  BecomeConfig     │    │ become_user: Opt  │    │  Option<Str>  │
│                   │    │ become_method:Opt │    │               │
│ (+ all other      │    │                   │    │ (+ cwd, env,  │
│  execution ctx)   │    │ (+ all other      │    │  timeout)     │
└───────────────────┘    │  module ctx)      │    └───────────────┘
                         └───────────────────┘

┌────────────────────────────────────────────────────────────────┐
│                    Conversion Flow                              │
└────────────────────────────────────────────────────────────────┘

  BecomeConfig { enabled: true, user: "admin" }
        │
        ├──────────────────┬───────────────────┐
        │                  │                   │
        ▼                  ▼                   ▼
ModuleContext {    ExecuteOptions {    SSH Command Wrapper
  become: true,      escalation:         │
  become_user:         Some("admin")     ▼
    Some("admin")    }               "sudo -u admin <cmd>"
}
```

## Error Flow Diagram

```
┌────────────────────────────────────────────────────────────────┐
│                    Error Handling Paths                         │
└────────────────────────────────────────────────────────────────┘

                        Module requested
                               │
                               ▼
                    ┌──────────────────────┐
                    │  Is native module?   │
                    └──────┬───────────────┘
                           │
                    ┌──────┴──────┐
                  YES             NO
                    │              │
                    ▼              ▼
            ┌───────────┐   ┌──────────────────┐
            │Execute    │   │ Find in Ansible  │
            │natively   │   │ module paths     │
            └─────┬─────┘   └────┬─────────────┘
                  │              │
                  │         ┌────┴─────┐
                  │       FOUND      NOT FOUND
                  │         │            │
                  │         ▼            ▼
                  │  ┌─────────────┐  ┌──────────────────┐
                  │  │Connection   │  │ ERROR:           │
                  │  │available?   │  │ ModuleNotFound   │
                  │  └──────┬──────┘  │                  │
                  │         │         │ "Module 'xyz'    │
                  │    ┌────┴────┐    │  not found."     │
                  │  YES        NO     │                  │
                  │    │         │    │ "Not native and  │
                  │    ▼         ▼    │  not in Ansible" │
                  │ ┌──────┐ ┌───────┐│                  │
                  │ │Exec  │ │ERROR: ││ "Install Ansible"│
                  │ │Python│ │ConnReq││ "Set ANSIBLE_LIB"│
                  │ └───┬──┘ └───┬───┘│                  │
                  │     │        │    │ Searched:        │
                  │     │        │    │ - /usr/share/... │
                  │     │        │    │ - ~/.ansible/... │
                  │     ▼        ▼    └──────────────────┘
                  │  SUCCESS  FAILURE
                  │     │        │
                  └─────┴────────┴──────────────┐
                                                │
                                                ▼
                                    ┌───────────────────┐
                                    │  Return to caller │
                                    │  - TaskResult     │
                                    │  - Or Error       │
                                    └───────────────────┘

Error Types (NEW):
┌──────────────────────────────────────────────────────────────┐
│ ExecutorError::ModuleNotFound(String)                        │
│   Contains:                                                  │
│     - Module name                                            │
│     - Searched paths                                         │
│     - Fix suggestions (install Ansible, set ANSIBLE_LIBRARY) │
│                                                              │
│ ExecutorError::ConnectionRequired(String)                   │
│   Contains:                                                  │
│     - Module name                                            │
│     - Reason (Python module, remote execution)               │
│                                                              │
│ ExecutorError::ModuleExecutionFailed(String)                │
│   Contains:                                                  │
│     - Module name                                            │
│     - Error details                                          │
│     - Stdout/stderr if available                             │
└──────────────────────────────────────────────────────────────┘
```

## State Transition Diagram: Module Execution

```
┌────────────────────────────────────────────────────────────────┐
│              Module Execution State Machine                     │
└────────────────────────────────────────────────────────────────┘

                        ┌─────────┐
                        │ PENDING │
                        └────┬────┘
                             │
                             │ Task::execute()
                             ▼
                        ┌─────────┐
                        │RESOLVING│
                        │ BECOME  │
                        └────┬────┘
                             │
                             │ BecomeConfig::resolve()
                             ▼
                        ┌─────────┐
                        │EXECUTING│◀───┐
                        │ MODULE  │    │
                        └────┬────┘    │
                             │         │
              ┌──────────────┼──────────────┐
              │              │              │
          NATIVE         PYTHON        CHECK MODE
              │              │              │
              ▼              ▼              ▼
         ┌─────────┐    ┌─────────┐   ┌─────────┐
         │ NATIVE  │    │ PYTHON  │   │ SKIPPED │
         │  EXEC   │    │  EXEC   │   │         │
         └────┬────┘    └────┬────┘   └────┬────┘
              │              │              │
              │              │              │
              └──────────────┼──────────────┘
                             │
                             ▼
                        ┌─────────┐
                        │COMPLETED│
                        └────┬────┘
                             │
              ┌──────────────┼──────────────┐
              │              │              │
          SUCCESS          FAILURE       SKIPPED
              │              │              │
              ▼              ▼              ▼
         ┌─────────┐    ┌─────────┐   ┌─────────┐
         │TaskResult│    │TaskResult│   │TaskResult│
         │ changed │    │ failed  │   │ skipped │
         │ rc=0    │    │ rc≠0    │   │         │
         └─────────┘    └─────────┘   └─────────┘

REMOVED STATES (eliminated by this design):
  ❌ SIMULATING (fake execution)
  ❌ STUBBED (debug-only)
  ❌ WOULD_EXECUTE (logging without action)
```

---

## Legend

```
┌────────────────────────────────────────────────────────────────┐
│                    Diagram Notation                             │
└────────────────────────────────────────────────────────────────┘

  ┌─────┐
  │ Box │         Component or entity
  └─────┘

  ────▶           Data flow or control flow

  ═══▶           Emphasized/important flow

  ┌─────┐
  │  ?  │         Decision point
  └──┬──┘
     ├─── Yes
     └─── No

  ✅              Correct/implemented behavior
  ❌              Incorrect/removed behavior

  NEW             Newly created component
  MODIFIED        Changed component
  REMOVED         Deleted component

  // comment      Inline explanation
```

---

## Quick Reference: File Locations

```
New files:
  src/executor/become.rs              BecomeConfig type and resolution logic
  tests/unit/executor/become_config.rs     Unit tests for become
  tests/integration/become.yml        Integration test playbook

Modified files:
  src/executor/runtime.rs             ExecutionContext with become field
  src/executor/task.rs                Become resolution, real module execution
  src/connection/mod.rs               ExecuteOptions helpers for become
  src/cli/commands/run.rs             CLI become integration

Removed code paths:
  src/executor/task.rs:1261-1275      Simulated command execution
  src/executor/task.rs:1475-1597      Stubbed modules (package, service, etc.)
  src/executor/task.rs:1034-1037      Python module simulation without connection
```
