---
summary: Guide to running Rustible in containers using Docker and Kubernetes, including image building, compose setups, and CI/CD integration.
read_when: You want to run Rustible in Docker containers or deploy to Kubernetes.
---

# Rustible Container Guide

This guide covers running Rustible in containers using Docker and Kubernetes.

## Table of Contents

- [Docker Quick Start](#docker-quick-start)
- [Building the Image](#building-the-image)
- [Running Rustible](#running-rustible)
- [Docker Compose](#docker-compose)
- [Kubernetes Deployment](#kubernetes-deployment)
- [CI/CD Integration](#cicd-integration)
- [Best Practices](#best-practices)

## Docker Quick Start

### Pull the Image

```bash
# Latest stable release
docker pull ghcr.io/rustible/rustible:latest

# Specific version
docker pull ghcr.io/rustible/rustible:0.1.0
```

### Run a Playbook

```bash
# Run a playbook from your local directory
docker run --rm \
  -v $(pwd)/playbooks:/workspace/playbooks:ro \
  -v $(pwd)/inventory:/workspace/inventory:ro \
  -v ~/.ssh:/home/rustible/.ssh:ro \
  ghcr.io/rustible/rustible:latest \
  run /workspace/playbooks/site.yaml \
  -i /workspace/inventory/hosts.yaml
```

## Building the Image

### Local Build

```bash
# Build with default settings (pure-rust features)
docker build -t rustible:local .

# Build with all features
docker build -t rustible:full \
  --build-arg FEATURES="full" .

# Build for specific platform
docker build -t rustible:arm64 \
  --platform linux/arm64 .
```

### Multi-Architecture Build

```bash
# Create builder for multi-arch
docker buildx create --name rustible-builder --use

# Build and push multi-arch image
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -t ghcr.io/rustible/rustible:latest \
  --push .
```

## Running Rustible

### Basic Usage

```bash
# Show version
docker run --rm ghcr.io/rustible/rustible:latest --version

# Show help
docker run --rm ghcr.io/rustible/rustible:latest --help

# Check playbook syntax
docker run --rm \
  -v $(pwd):/workspace:ro \
  ghcr.io/rustible/rustible:latest \
  check /workspace/playbook.yaml
```

### With SSH Keys

```bash
# Mount SSH keys for remote execution
docker run --rm \
  -v ~/.ssh/id_ed25519:/home/rustible/.ssh/id_ed25519:ro \
  -v ~/.ssh/known_hosts:/home/rustible/.ssh/known_hosts:ro \
  -v $(pwd):/workspace:ro \
  ghcr.io/rustible/rustible:latest \
  run /workspace/playbook.yaml -i /workspace/inventory.yaml
```

### With Vault Password

```bash
# Using environment variable
docker run --rm \
  -e RUSTIBLE_VAULT_PASSWORD='your-vault-password' \
  -v $(pwd):/workspace:ro \
  ghcr.io/rustible/rustible:latest \
  run /workspace/playbook.yaml --ask-vault-pass

# Using password file
docker run --rm \
  -v $(pwd)/vault-password:/workspace/.vault-password:ro \
  -v $(pwd):/workspace:ro \
  ghcr.io/rustible/rustible:latest \
  run /workspace/playbook.yaml --vault-password-file /workspace/.vault-password
```

### Interactive Mode

```bash
# Start interactive shell
docker run -it --rm \
  -v $(pwd):/workspace \
  --entrypoint /bin/bash \
  ghcr.io/rustible/rustible:latest
```

## Docker Compose

### Development Setup

```yaml
# docker-compose.yaml
version: '3.8'

services:
  rustible:
    image: ghcr.io/rustible/rustible:latest
    volumes:
      - ./playbooks:/workspace/playbooks:ro
      - ./inventory:/workspace/inventory:ro
      - ~/.ssh:/home/rustible/.ssh:ro
    environment:
      - RUST_LOG=info
    entrypoint: ["/bin/bash"]
    stdin_open: true
    tty: true
```

### Testing Environment

Use the provided docker-compose.yml in `deploy/docker/`:

```bash
cd deploy/docker

# Start development environment
docker compose up -d

# Start with test targets
docker compose --profile testing up -d

# Run a playbook against test targets
docker compose exec rustible rustible run \
  /workspace/examples/playbook.yaml \
  -i /workspace/inventory.yaml

# Cleanup
docker compose down -v
```

## Kubernetes Deployment

### Prerequisites

1. Kubernetes cluster (1.25+)
2. kubectl configured
3. Container registry access

### Quick Deploy

```bash
# Apply all manifests
kubectl apply -k deploy/kubernetes/

# Verify deployment
kubectl get pods -n rustible

# View logs
kubectl logs -n rustible deployment/rustible
```

### Configure Secrets

Before deploying, update the secrets:

```bash
# Generate SSH key
ssh-keygen -t ed25519 -f id_ed25519 -N ""

# Create secret from files
kubectl create secret generic rustible-ssh-keys \
  -n rustible \
  --from-file=id_ed25519=./id_ed25519 \
  --from-file=id_ed25519.pub=./id_ed25519.pub

# Create vault password secret
kubectl create secret generic rustible-vault \
  -n rustible \
  --from-literal=vault-password='your-vault-password'
```

### Run Ad-hoc Playbook

```bash
# Create a job from template
kubectl apply -f - <<EOF
apiVersion: batch/v1
kind: Job
metadata:
  name: rustible-adhoc-$(date +%s)
  namespace: rustible
spec:
  template:
    spec:
      containers:
        - name: rustible
          image: ghcr.io/rustible/rustible:latest
          command: ["/usr/local/bin/rustible"]
          args: ["run", "/workspace/playbooks/myplaybook.yaml", "-v"]
          volumeMounts:
            - name: playbooks
              mountPath: /workspace/playbooks
      volumes:
        - name: playbooks
          configMap:
            name: my-playbooks
      restartPolicy: Never
EOF
```

### Scheduled Execution

The CronJob runs playbooks on a schedule:

```bash
# Check CronJob status
kubectl get cronjobs -n rustible

# View job history
kubectl get jobs -n rustible

# Check last run logs
kubectl logs -n rustible job/rustible-scheduled-xxxxx
```

## CI/CD Integration

### GitHub Actions

The project includes a Docker workflow (`.github/workflows/docker.yml`) that:

1. Builds the Docker image on every push/PR
2. Runs security scans with Trivy
3. Tests the image functionality
4. Pushes to GitHub Container Registry on main branch
5. Creates multi-arch images for releases

### GitLab CI

```yaml
# .gitlab-ci.yml example
docker-build:
  image: docker:24
  services:
    - docker:24-dind
  script:
    - docker build -t $CI_REGISTRY_IMAGE:$CI_COMMIT_SHA .
    - docker push $CI_REGISTRY_IMAGE:$CI_COMMIT_SHA
```

## Best Practices

### Security

1. **Run as non-root**: The image runs as user `rustible` (UID 1000)
2. **Read-only mounts**: Mount playbooks and inventory as read-only
3. **Secret management**: Use Kubernetes secrets or Docker secrets for SSH keys
4. **Minimal image**: Based on `debian:bookworm-slim` with only required packages
5. **Security scanning**: Enable Trivy scans in CI/CD

### Performance

1. **Layer caching**: Use cargo-chef for efficient dependency caching
2. **Multi-stage builds**: Separate build and runtime stages
3. **Resource limits**: Set appropriate CPU/memory limits in Kubernetes
4. **Build args**: Use `CARGO_BUILD_JOBS` for parallel compilation

### Networking

1. **SSH access**: Ensure container can reach target hosts
2. **DNS resolution**: Configure Kubernetes DNS for service discovery
3. **Network policies**: Restrict egress to known hosts in production

### Volumes

1. **Workspace**: Mount playbooks and inventory as volumes
2. **SSH keys**: Mount as read-only with proper permissions (0600)
3. **Output**: Use emptyDir or PVC for generated files

## Troubleshooting

### Image Won't Build

```bash
# Check build logs
docker build --no-cache --progress=plain -t rustible:debug .

# Verify Rust dependencies
docker run --rm rust:1.82 cargo check
```

### SSH Connection Issues

```bash
# Test SSH from container
docker run -it --rm \
  -v ~/.ssh:/home/rustible/.ssh:ro \
  --entrypoint /bin/bash \
  ghcr.io/rustible/rustible:latest \
  -c "ssh -v user@host hostname"
```

### Permission Denied

```bash
# Check SSH key permissions in container
docker run --rm \
  -v ~/.ssh:/home/rustible/.ssh:ro \
  --entrypoint /bin/bash \
  ghcr.io/rustible/rustible:latest \
  -c "ls -la /home/rustible/.ssh/"
```

### Pod Not Starting in Kubernetes

```bash
# Check pod events
kubectl describe pod -n rustible <pod-name>

# Check container logs
kubectl logs -n rustible <pod-name> --previous
```

## Image Details

| Property | Value |
|----------|-------|
| Base Image | `debian:bookworm-slim` |
| User | `rustible` (UID 1000) |
| Working Directory | `/workspace` |
| Entrypoint | `/usr/local/bin/rustible` |
| Exposed Ports | None |
| Architectures | `linux/amd64`, `linux/arm64` |

## Related Files

- `/Dockerfile` - Multi-stage build definition
- `/deploy/docker/docker-compose.yml` - Development/testing compose file
- `/deploy/kubernetes/` - Kubernetes manifests
- `/.github/workflows/docker.yml` - CI/CD workflow
- `/.dockerignore` - Build context exclusions
