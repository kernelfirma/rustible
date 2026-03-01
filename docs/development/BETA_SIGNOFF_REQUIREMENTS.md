# Beta Sign-Off Requirements

Rustible beta releases must pass the dedicated high-risk workflow:

- Workflow: `.github/workflows/high-risk-suites.yml`
- Required profile: `beta-signoff`

## Required Suites for Beta Sign-Off

| Suite | Workflow Job | Requirement |
|---|---|---|
| SSH integration, stress, chaos | `ssh` | Must run in `full` mode (`real_ssh_tests`, `parallel_stress_tests`, `chaos_tests`) |
| WinRM parity + integration | `winrm` | Must run in `full` mode (`winrm_parity_tests` and `winrm_integration_tests --ignored`) |
| HPC scale validation | `hpc` | Must pass `hpc_scale_validation_tests --ignored` with `hpc` feature |
| Fuzz regression | `fuzz` | Must pass `callback_fuzz_tests` and `proptest_tests` with bounded `PROPTEST_CASES` |

If `ssh` or `winrm` falls back to `smoke` mode during `beta-signoff`, the workflow fails.

## CI Inputs for Full Coverage

Configure these repository variables/secrets so CI can run full infrastructure-backed suites:

### SSH full mode

- Variable: `CI_SSH_HOSTS` (comma-separated hosts)
- Variable: `CI_SSH_USER`
- Optional variable: `CI_SCALE_HOSTS`
- Optional variable: `CI_TEST_INVENTORY`
- Secret: `CI_SSH_PRIVATE_KEY` (PEM/OpenSSH private key)
- Optional secret: `CI_SSH_PASSWORD`

### WinRM full mode

- Variable: `CI_WINRM_HOST`
- Variable: `CI_WINRM_USER`
- Optional variable: `CI_WINRM_PORT` (default `5985`)
- Optional variable: `CI_WINRM_SSL` (`true` or `false`, default `false`)
- Secret: `CI_WINRM_PASS`

## Running the Workflow

### From GitHub Actions UI

1. Open `High-Risk Suite Validation`.
2. Click **Run workflow**.
3. Set `signoff_profile=beta-signoff`.
4. Optionally set `proptest_cases` (default for beta sign-off is `512`).

### From CLI

```bash
gh workflow run high-risk-suites.yml \
  --ref main \
  -f signoff_profile=beta-signoff \
  -f enforce_full_infra=true \
  -f proptest_cases=512
```

## Scheduled Coverage

A weekly schedule runs the workflow in `scheduled-smoke` mode.

- SSH/WinRM use full mode when infrastructure variables/secrets are present.
- If infrastructure is unavailable, those suites run deterministic smoke fallbacks with explicit summary reporting.
- HPC and fuzz suites still run fully on schedule.
