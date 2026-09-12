# Diagnostics

Diagnostics are actionable and preserve source location when available.

## Config parse errors

Format: `path:line:column: message`.

Example:

```text
config/project.jsonc:4:17: expected comma
```

Malformed JSONC, missing `schema_version`, invalid schema values, and unsupported future schema versions are parse diagnostics.

## Unknown fields

Unknown config fields identify path, line, and field name:

```text
config/project.jsonc:5: unknown field `modle`
```

Remove the field or use a supported schema field.

## Secrets

Plaintext `api_key` values are rejected. Use `env:NAME` or `credential:ENTRY_ID`.

```text
config/project.jsonc: plaintext secret `api_key` is not allowed; use env: or credential: reference
```

Missing environment variables identify the requested variable. Credential-store lookup is explicit when the platform adapter is unavailable.

## Workspace and approval

Workspace diagnostics identify rejected paths, directory targets, duplicate mutation paths, snapshot failures, and policy decisions. Review the diff, then retry BUILD with approval when operation risk requires it.

PLAN rejection is expected for mutation operations; switch to BUILD only after reviewing the proposed change.

## Provider and runtime failures

Provider errors remain isolated from other providers. Cached/static models remain usable when discovery fails. Cancellation is reported as terminal `Cancelled`; it is not reported as successful completion.
