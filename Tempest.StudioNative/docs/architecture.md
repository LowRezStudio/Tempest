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
  as bytes; `WORK`/`Busy` count work in flight; `twrk.save` reads the editors on the backend thread,
`Server::twrk_save_job`, and the `twrk-saver` thread makes the deltas and writes the file, one save at
a time, saves of the same file queued behind it written once from the newest), so it calls the same methods as the Electron Studio
  without IPC. Ported view by view while the Electron Studio keeps shipping. The app runs on
  mimalloc like `upk` (`src/main.rs`): the backend gives the memory freed after opening a folder
  back to the system (`release_memory`), which macOS's allocator would keep (8.1 open: ~0.57 GB
  instead of ~1.25 GB).
