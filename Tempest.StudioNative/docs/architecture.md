# Native Studio: architecture

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Native Studio architecture

- `Tempest.StudioNative/` — the Studio being rewritten as a native **GPUI** app (`gpui-component`
  0.9 on `gpui-pre` 0.3.11 + `gpui-pre-platform`, Lucide icons from `gpui-kit-assets` registered in
  `src/assets.rs`), a standalone Cargo package with its own `rust-toolchain.toml` (1.99.1:
  `gpui-pre` needs a newer Rust than 1.97). It runs `upk serve`'s `Server` in-process on a worker
  thread (`src/backend.rs`: `Server::with_events` + `call_bin`, replies keep their binary payloads
  as bytes; `WORK`/`Busy` count work in flight), so it calls the same methods as the Electron Studio
  without IPC. Ported view by view while the Electron Studio keeps shipping.
