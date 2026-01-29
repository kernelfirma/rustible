---
summary: Reference for the uri module that performs HTTP requests.
read_when: You need to call HTTP/HTTPS endpoints from playbooks.
---

# uri - Perform HTTP Requests

## Synopsis

The `uri` module performs HTTP/HTTPS requests to APIs and web services. It can send
GET, POST, PUT, and DELETE requests with custom headers and payloads.

## Classification

**RemoteCommand** - Executes network requests from the control flow context.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `url` | yes | - | string | Target URL to request. |
| `method` | no | GET | string | HTTP method (GET, POST, PUT, DELETE). |
| `body` | no | - | string | Request body for POST/PUT. |
| `headers` | no | - | map | HTTP headers to include. |
| `status_code` | no | 200 | int | Expected status code(s). |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `status` | int | HTTP status code. |
| `body` | string | Response body. |

## Examples

### Simple GET request

```yaml
- name: Fetch health endpoint
  uri:
    url: https://api.example.com/health
```

### POST JSON payload

```yaml
- name: Create resource
  uri:
    url: https://api.example.com/items
    method: POST
    headers:
      Content-Type: application/json
    body: '{"name": "demo"}'
    status_code: 201
```

## Notes

- Use `status_code` to validate expected responses.
- For file downloads, prefer `get_url` when available.
