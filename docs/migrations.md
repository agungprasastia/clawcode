# Configuration Migrations

## Current schema

Current config schema: `1`.

Every config file must contain numeric `schema_version`.

## Load order

1. Parse global config.
2. Parse project config.
3. Migrate each supported older version to current schema.
4. Merge project values over global values.
5. Resolve secrets only when needed.

Missing project values inherit global values. Project values do not mutate the global file.

## Migration rules

The loader accepts schema versions at or below current and returns a config with `schema_version: 1`. Unsupported future versions fail safely with a diagnostic; no file is rewritten automatically.

Before changing a config manually:

1. Copy the file as a backup.
2. Preserve `schema_version`.
3. Replace plaintext secrets with `env:` or `credential:` references.
4. Run the config tests or start with `--version` to check basic startup.

Schema migrations are intentionally conservative. A future schema change must add a tested conversion step and retain actionable source diagnostics.
