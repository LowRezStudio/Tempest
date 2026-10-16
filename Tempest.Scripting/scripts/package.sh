#!/usr/bin/env bash
# Build Tempest.Scripting64.dll for the Win64 client from macOS/Linux and
# stage four installable packages (same format as `tempest mod pack`: a zip
# of the mod folder with manifest.toml at the root plus a CHECKSUMS file):
#
#   dist/scripting.tempest   runtime: the DLL, core console commands, editor types
#   dist/console.tempest     enables the in-game console (F2)
#   dist/dev.tempest         whatever is in mods/dev/ — the scratch mod being worked on
#   dist/lib.tempest         example library mod (exports `greet` for dev's 07_shared.lua)
#
# Uses the local MinGW-w64 toolchain when x86_64-w64-mingw32-gcc is on PATH
# (`brew install mingw-w64`), otherwise falls back to `cross` (needs Docker).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OUT="${1:-dist}"
case "$OUT" in /*) ;; *) OUT="$ROOT/$OUT" ;; esac
TARGET="x86_64-pc-windows-gnu"

if command -v x86_64-w64-mingw32-gcc >/dev/null; then
    echo "building with local MinGW-w64"
    cargo build --release --target "$TARGET" -p tempest-scripting
else
    command -v cross >/dev/null || { echo "need x86_64-w64-mingw32-gcc (brew install mingw-w64) or cross (cargo install cross)" >&2; exit 1; }
    echo "building with cross"
    [[ "$(uname -m)" == "arm64" ]] && export DOCKER_DEFAULT_PLATFORM=linux/amd64
    cross build --release --target "$TARGET" -p tempest-scripting
fi

DLL="target/$TARGET/release/tempest_scripting.dll"
# The DLL is useless without its entry point; fail loudly if it went missing.
if command -v x86_64-w64-mingw32-objdump >/dev/null; then
    exports="$(x86_64-w64-mingw32-objdump -p "$DLL")"
    grep -qE '[[:space:]]DllMain[[:space:]]*$' <<<"$exports" \
        || { echo "ERROR: $DLL does not export DllMain" >&2; exit 1; }
fi

mkdir -p "$OUT"
cp "$DLL" "$OUT/Tempest.Scripting64.dll"


# pack <staged folder> <out.tempest>: CHECKSUMS ("<sha256> <relpath>" lines,
# ordinal order, LF-joined, no trailing newline) + zip, like Tempest.CLI.
pack() {
    python3 "$ROOT/scripts/pack.py" "$1" "$2"
    echo "packed: $2"
}

LUARC='{
  "runtime.version": "LuaJIT",
  "workspace.library": ["types"],
  "workspace.checkThirdParty": false,
  "diagnostics.globals": ["MOD", "shared"]
}'

# --- scripting.tempest: the runtime --------------------------------------
S="$OUT/scripting"
rm -rf "$S"
cp -R mods/scripting "$S"
mkdir -p "$S/dlls" "$S/types"
cp "$OUT/Tempest.Scripting64.dll" "$S/dlls/"
cp types/tempest.d.lua "$S/types/"
printf '%s\n' "$LUARC" > "$S/.luarc.json"
pack "$S" "$OUT/scripting.tempest"

# --- console.tempest ------------------------------------------------------
C="$OUT/console"
rm -rf "$C"
cp -R mods/console "$C"
pack "$C" "$OUT/console.tempest"

# --- dev.tempest: the scratch mod ------------------------------------------
D="$OUT/dev"
rm -rf "$D"
cp -R mods/dev "$D"
mkdir -p "$D/types"
cp types/tempest.d.lua "$D/types/"
printf '%s\n' "$LUARC" > "$D/.luarc.json"
pack "$D" "$OUT/dev.tempest"

# --- lib.tempest: example library mod ----------------------------------------
L="$OUT/lib"
rm -rf "$L"
cp -R mods/lib "$L"
pack "$L" "$OUT/lib.tempest"

echo "staged: $OUT/Tempest.Scripting64.dll"
