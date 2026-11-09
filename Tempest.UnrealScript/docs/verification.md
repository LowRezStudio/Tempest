# UnrealScript: verification

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Verification (the harness)

There are almost no unit tests. The test is the game:

```bash
scripts/verify.sh ~/Games/Tempest/0.57/ChaosGame/CookedPCConsole     # or: just unrealscript-verify <dir>
```

It runs `project init` + `project build` into `target/verify/` and prints `<Pkg>: N diff lines` per
package from `uscript dump --hash` of original vs rebuilt. **Every cooked game package must report
0.** Packages that are not cooked (the user's UDK-built mods TmCore/TempestMp) differ in a few
functions because UDK compiled them against UDK's Engine (e.g. `Canvas.DrawText` has one more
optional parameter in the game); that is expected and not a regression.

To investigate a mismatch:

```bash
U=target/release/uscript; C=<cooked dir>; W=target/verify
diff $W/dumps/TgGame.orig $W/dumps/TgGame.ours | head          # which exports, which tokens
$U dump $C/TgGame.upk a.txt -I $C --no-pos --hash-full          # full payload hex when the token dump matches but bytes differ
$U info|names|imports|exports $C/TgGame.upk                     # header tables (strip ` @offset` with sed before diffing)
```

Fast single-package loop while iterating: `uscript decompile` → edit nothing → `uscript compile ...
--cooked --strip-source --allow-config-defaults --compress --conform <orig>` → `uscript dump --hash`
both and diff. Always run the full verify on 0.57 before finishing, and on the 8.1 set when it is
available; the two versions exercise different code.
