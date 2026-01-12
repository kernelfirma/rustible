# Declarative Resource Graph Model

## Status
Draft

## Problem Statement
Rustible today uses imperative playbooks and task ordering. To support declarative workflows (Terraform-like), we need a minimal resource graph model that can be mapped onto the existing DAG executor while keeping playbooks first-class.

## Goals
- Define a minimal resource schema that is stable and easy to extend.
- Map resources to the existing executor DAG with deterministic ordering.
- Allow playbooks and resource graphs to coexist in one execution plan.

## Non-Goals
- Replace playbooks or remove imperative tasks.
- Implement a full Terraform-compatible DSL in the first iteration.
- Model every possible cloud provider resource in v0.

## Minimal Resource Schema
Each resource is an instance of a provider-defined type with attributes and dependencies.

```yaml
resources:
  - id: web_server
    type: aws_instance
    desired:
      instance_type: t3.micro
      ami: ami-12345
      tags:
        Name: web-1
    depends_on: [vpc_main, subnet_web]
    lifecycle:
      create_before_destroy: true
      ignore_changes: ["tags.Generated"]

  - id: config_app
    type: playbook
    desired:
      path: playbooks/app.yml
      vars:
        app_port: 8080
    depends_on: [web_server]
```

### Fields
- `id`: Unique, stable identifier used for graph edges.
- `type`: Resource type (provider-specific or built-in types like `playbook`).
- `desired`: Desired state payload (opaque to core, validated by provider).
- `depends_on`: Explicit dependencies between resources.
- `lifecycle`: Optional behavior toggles (create_before_destroy, ignore_changes).

## DAG Mapping
- Each resource becomes a DAG node with edges defined by `depends_on`.
- Providers expose a `plan` phase that yields `create/update/delete` actions.
- Each action becomes a subnode under the resource node; order is
  `plan -> apply -> verify`.
- Existing playbook execution is modeled as a single `apply` action node.

## Coexistence with Playbooks
- Playbooks remain the default entrypoint.
- A resource graph can be embedded in a playbook via a `resource_graph` task
  or executed as a separate CLI subcommand.
- Variables flow from resource outputs into playbook vars via `outputs`.

## Provider Responsibilities
- Validate `desired` payloads.
- Implement `plan` and `apply` semantics.
- Emit resource outputs for downstream tasks.

## Next Steps
- Define the core `Resource` and `ResourceGraph` structs.
- Add a prototype CLI entrypoint (`rustible graph apply`).
- Implement a single provider with a stub resource type.
