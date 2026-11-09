# Launcher development

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Launcher commands

Run from the repository root (the root `package.json` is the workspace for both Electron apps; `just
js-*` wraps these):

```bash
corepack enable        # one-time
pnpm install           # installs both apps (root pnpm-workspace.yaml)

# Dev desktop app (Electron + Vite)
# Build Tempest.CLI first; dev commands run it directly via `dotnet exec`
dotnet build Tempest.slnx
pnpm -C Tempest.Launcher electron:dev
# Vite dev server is fixed to http://localhost:1420

# Production build (bundles appear in Tempest.Launcher/dist/)
pnpm -C Tempest.Launcher electron:build

# Type-check / Svelte-check (the launcher's scripts, or `pnpm check:launcher` from the root)
pnpm -C Tempest.Launcher check
pnpm -C Tempest.Launcher check:electron

# Format every JS package (each app's own `fmt`: oxfmt, root .oxfmtrc.json)
pnpm fmt

# Lint every JS package (each app's own `lint`: oxlint --type-aware, root .oxlintrc.json)
pnpm lint

# Regenerate TS protobuf clients from Tempest.Protocol
pnpm -C Tempest.Launcher proto-generate
```
