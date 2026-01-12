# Architecture Documentation Index

This index lists all architecture-related documents in this directory.

## Architecture Decision Records (ADRs)

| File | Status | Summary |
|------|--------|---------|
| [0001-architecture-overview.md](./0001-architecture-overview.md) | Accepted | Core design layers, async execution model, and Ansible compatibility strategy |
| [0002-module-system-design.md](./0002-module-system-design.md) | Accepted | Module trait, execution tiers, check mode, diff support, and idempotency |
| [0003-callback-plugin-architecture.md](./0003-callback-plugin-architecture.md) | Accepted | Callback plugin system for execution events, output formatting, and integrations |

## Architecture Documents

| File | Status | Summary |
|------|--------|---------|
| [ARCHITECTURE.md](./ARCHITECTURE.md) | Current | Internal architecture overview with async-first design, module tiers, and extension points |
| [ARCHITECTURE_REVIEW_02.md](./ARCHITECTURE_REVIEW_02.md) | Complete | Full codebase architecture analysis with module cohesion ratings |
| [REGISTRY_ARCHITECTURE.md](./REGISTRY_ARCHITECTURE.md) | Draft | Package registry system design as modern Ansible Galaxy replacement |
| [provider-ecosystem.md](./provider-ecosystem.md) | Draft | Provider SDK and registry distribution model |

## Feature Design Documents

| File | Status | Summary |
|------|--------|---------|
| [distributed-execution.md](./distributed-execution.md) | Draft | Multi-controller architecture for horizontal scaling to 10,000+ hosts |
| [terraform-integration.md](./terraform-integration.md) | Draft | Dynamic inventory from Terraform state and bidirectional variable sharing |
| [web-ui.md](./web-ui.md) | Draft | Browser-based management console with live job output streaming |
| [resource-graph-model.md](./resource-graph-model.md) | Draft | Declarative resource graph model and DAG mapping |
| [awx-vault-integration.md](./awx-vault-integration.md) | Draft | AWX/Tower API compatibility scope and Vault integration plan |
| [ansible-compat-gap.md](./ansible-compat-gap.md) | Draft | Compatibility gap inventory and test plan |

## Implementation Specifications

| File | Status | Summary |
|------|--------|---------|
| [aws-resource-roadmap.md](./aws-resource-roadmap.md) | Complete | Next 5-10 AWS resources prioritized for Terraform replacement |
| [IMPLEMENTATION_SPEC.md](./IMPLEMENTATION_SPEC.md) | Draft | Phase 1 specification for aws_iam_role, aws_ebs_volume, and related resources |

## P0 Features (M1 Milestone)

| File | Status | Summary |
|------|--------|---------|
| [p0-features-design.md](./p0-features-design.md) | Design | Comprehensive design for become and simulated execution removal |
| [p0-features-diagrams.md](./p0-features-diagrams.md) | Design | C4 system context and container diagrams for P0 features |
| [p0-features-implementation-guide.md](./p0-features-implementation-guide.md) | Design | Concrete code examples for implementing Issues #52 and #53 |
| [p0-features-summary.md](./p0-features-summary.md) | Design | Quick reference for P0 feature implementation |
