# faststart

[Agent entry point](../AGENTS.md). Source: [`src/faststart.zig`](../src/faststart.zig).

`faststart-windows_x86_64.dll` shortens the time the Paladins 8.1 Win64 client takes to reach
the login screen. The CLI injects it right after the process starts (`launch`, client only);
`--no-fast-startup` leaves it out, and so does `--no-dlls`.

Nothing in it changes what the game computes. Every patch either answers a question the game
asks again and again with the answer it already got, or does the same work on more threads.

## Build gating

The DLL only patches the build it was written for, checked twice:

- the CLI injects it only when the executable is 64-bit and carries the 8.1 link timestamp
  (`FastStartup.Supports`);
- the DLL checks the loaded image's timestamp and size again, and every site's expected bytes
  before it writes or detours. A site that does not match is skipped and logged.

Supporting another build means a new address table, not loosening those checks.

## Patches

Each has a name that `TEMPEST_FASTSTART_DISABLE` accepts (comma-separated, or `all`).

| Name | What it does |
| --- | --- |
| `sleeps` | Shortens three fixed waits on the startup path (voice init, thread naming, pool worker). |
| `rand` | Seeds OpenSSL from the system generator instead of the slow heap/process walk of `RAND_poll`. |
| `file_size` | Answers the file manager's size query with one attributes call for cooked-folder paths and bare names, and remembers each answer for 250 ms. |
| `packages` | Remembers package names that were just looked up and not found (2 s), so the same miss is not searched thousands of times in a row. Hits are never remembered. |
| `chunks` | Keeps decompressed package chunks that are read a second time (128 MiB, least recently used out first, released after 20 s idle). Keyed by file identity, size and write time. |
| `inflate` | On a chunk the cache does not have: reads all its blocks in one read, undoes the byte mask 32 bytes at a time, and decompresses the blocks on several threads with the game's own routine. Needs `chunks`. Falls back to the game's code on anything unexpected. |
| `graphics` | Loads DXGI, D3D11 and D3D9 on a second thread while packages load, so the renderer's first calls find them ready. |
| `crc` | Slicing-by-8 version of the engine's memory CRC, compared against the original before it is used. |

## Environment

- `TEMPEST_FASTSTART_DISABLE=<names>`: skip those patches (for bisecting a problem).
- `TEMPEST_FASTSTART_PROFILE=<seconds>`: sample the main thread every millisecond for that
  long and log inclusive/self counts per game function (RVA) in 2 s windows.

## Log

`%TEMP%\tempest-faststart.log` (in a Wine prefix: `drive_c/users/<user>/AppData/Local/Temp`).
It lists which patches went in, then cache statistics every 5 s while they change.

## Measuring

Use the game's own number: the `Initial startup` line of `Launch.log`. Compare against
`--no-fast-startup`, alternate the runs, and use a throwaway `--homedir` so the test does
not touch a real profile:

```sh
(sleep 40; echo kill) | dotnet run --project Tempest.CLI -- launch <game> --homedir FaststartTest
(sleep 50; echo kill) | dotnet run --project Tempest.CLI -- launch <game> --homedir FaststartTest --no-fast-startup
```

A debug CLI loads the DLL from `Tempest.Utils/zig-out/bin`, a published one from its own folder.

## Adding a patch

- Tie it to expected bytes at the site, give it a disable name, and log whether it went in.
- Keep the game's results identical; when a shortcut cannot be sure, call the original.
- Leave network behaviour alone (the startup HTTP request is slow but is not ours to reorder).
- Profile first (`TEMPEST_FASTSTART_PROFILE`), and keep a patch only if the alternating
  runs show it.
