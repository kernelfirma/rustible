# Notebook Deployment Makefile
# Simplifies common rustible operations

.PHONY: help check plan deploy deploy-packages deploy-dotfiles deploy-security clean test

# Default target
help:
	@echo "Notebook Deployment Commands:"
	@echo ""
	@echo "  make check              - Dry run (check mode)"
	@echo "  make plan               - Show execution plan"
	@echo "  make deploy             - Full deployment"
	@echo "  make deploy-packages    - Deploy only packages"
	@echo "  make deploy-dotfiles    - Deploy only dotfiles"
	@echo "  make deploy-security    - Deploy only security hardening"
	@echo "  make test               - Test connection to notebook"
	@echo "  make clean              - Clean up temporary files"
	@echo "  make ssh                - SSH to notebook"
	@echo ""

# Test SSH connection
test:
	@echo "Testing connection to notebook..."
	rustible all -i inventory.yml -m ping

# Dry run
check:
	@echo "Running dry-run deployment..."
	rustible run playbook.yml -i inventory.yml --check

# Show execution plan
plan:
	@echo "Showing execution plan..."
	rustible run playbook.yml -i inventory.yml --plan

# Full deployment
deploy:
	@echo "Starting full deployment..."
	rustible run playbook.yml -i inventory.yml

# Deploy specific roles
deploy-packages:
	@echo "Deploying packages..."
	rustible run playbook.yml -i inventory.yml --tags packages

deploy-dotfiles:
	@echo "Deploying dotfiles..."
	rustible run playbook.yml -i inventory.yml --tags dotfiles

deploy-security:
	@echo "Deploying security hardening..."
	rustible run playbook.yml -i inventory.yml --tags security

deploy-system:
	@echo "Deploying system services..."
	rustible run playbook.yml -i inventory.yml --tags system

deploy-graphics:
	@echo "Deploying graphics configuration..."
	rustible run playbook.yml -i inventory.yml --tags graphics

# Quick SSH to notebook
ssh:
	@ssh artur@$$(grep ansible_host inventory.yml | head -1 | awk '{print $$2}')

# Clean temporary files
clean:
	@echo "Cleaning temporary files..."
	rm -f *.retry
	rm -rf .rustible/
	@echo "Done"

# Verbose deployment for debugging
deploy-verbose:
	@echo "Starting verbose deployment..."
	rustible run playbook.yml -i inventory.yml -vvv

# Deploy with limits (specific host)
deploy-notebook:
	@echo "Deploying to notebook only..."
	rustible run playbook.yml -i inventory.yml --limit notebook
