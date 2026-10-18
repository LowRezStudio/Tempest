#!/usr/bin/env bash
# Build Tempest.Scripting64.dll (Win64 client) and Tempest.Scripting32.dll
# (Win32 client) from macOS/Linux and stage four installable packages (same
# format as `tempest mod pack`: a zip of the mod folder with manifest.toml at
# the root plus a CHECKSUMS file):
#
#   dist/scripting.tempest   runtime: both DLLs (dlls/64, dlls/32), core console commands, editor types
#   dist/console.tempest     enables the in-game console (F2)
#   dist/dev.tempest         whatever is in mods/dev/ — the scratch mod being worked on
#   dist/lib.tempest         example library mod (exports `greet` for dev's 07_shared.lua)
#
# The launcher injects dlls/64 into the Win64 client and dlls/32 into the
# Win32 one (DLLs directly in dlls/ go to both, the pre-split layout).
#
# Uses the local MinGW-w64 toolchains when x86_64-w64-mingw32-gcc /
# i686-w64-mingw32-gcc are on PATH (`brew install mingw-w64` ships both),
# otherwise falls back to `cross` (needs Docker). The 32-bit build also needs
# `wine` to run LuaJIT's host tools (see scripts/luajit-hostcc32.sh).
# TEMPEST_SCRIPTING_TARGETS="64" or "32" limits the build to one client.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OUT="${1:-dist}"
case "$OUT" in /*) ;; *) OUT="$ROOT/$OUT" ;; esac
TARGETS="${TEMPEST_SCRIPTING_TARGETS:-64 32}"

# build <bits>: cargo build for the client's target, verify the DLL, copy it to $OUT.
build() {
    local bits="$1" target gcc objdump
    if [[ "$bits" == 64 ]]; then
        target="x86_64-pc-windows-gnu"; gcc="x86_64-w64-mingw32-gcc"; objdump="x86_64-w64-mingw32-objdump"
    else
        target="i686-pc-windows-gnu"; gcc="i686-w64-mingw32-gcc"; objdump="i686-w64-mingw32-objdump"
    fi
    if command -v "$gcc" >/dev/null; then
        echo "building Win$bits with local MinGW-w64 ($gcc)"
        if [[ "$bits" == 32 ]]; then
            # LuaJIT's host tools must have the target's pointer size; with no
            # 32-bit host compiler they are built as i686 .exe and run under Wine.
            if [[ -z "${HOST_CC:-}" ]]; then
                command -v wine >/dev/null || { echo "the Win32 build needs wine (brew install --cask wine-stable) to run LuaJIT's host tools, or HOST_CC=\"gcc -m32\"" >&2; exit 1; }
                export HOST_CC="$ROOT/scripts/luajit-hostcc32.sh"
            fi
        fi
        cargo build --release --target "$target" -p tempest-scripting
    else
        command -v cross >/dev/null || { echo "need $gcc (brew install mingw-w64) or cross (cargo install cross)" >&2; exit 1; }
        echo "building Win$bits with cross"
        [[ "$(uname -m)" == "arm64" ]] && export DOCKER_DEFAULT_PLATFORM=linux/amd64
        cross build --release --target "$target" -p tempest-scripting
    fi

    local dll="target/$target/release/tempest_scripting.dll"
    # The DLL is useless without its entry point; fail loudly if it went missing
    # (the stdcall export is `DllMain@12` on Win32).
    if command -v "$objdump" >/dev/null; then
        local exports
        exports="$("$objdump" -p "$dll")"
        grep -qE '[[:space:]]DllMain(@12)?[[:space:]]*$' <<<"$exports" \
            || { echo "ERROR: $dll does not export DllMain" >&2; exit 1; }
        # The game folder has no MinGW runtime; everything must be static.
        if grep -qiE 'DLL Name: (libstdc\+\+|libgcc_s|libwinpthread)' <<<"$exports"; then
            echo "ERROR: $dll imports a MinGW runtime DLL:" >&2
            grep -iE 'DLL Name: (libstdc\+\+|libgcc_s|libwinpthread)' <<<"$exports" >&2
            exit 1
        fi
    fi
    mkdir -p "$OUT"
    cp "$dll" "$OUT/Tempest.Scripting$bits.dll"
}

for bits in $TARGETS; do
    build "$bits"
done


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
mkdir -p "$S/types"
for bits in $TARGETS; do
    mkdir -p "$S/dlls/$bits"
    cp "$OUT/Tempest.Scripting$bits.dll" "$S/dlls/$bits/"
done
cp types/tempest.d.lua types/imgui.d.lua "$S/types/"
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
cp types/tempest.d.lua types/imgui.d.lua "$D/types/"
printf '%s\n' "$LUARC" > "$D/.luarc.json"
pack "$D" "$OUT/dev.tempest"

# --- lib.tempest: example library mod ----------------------------------------
L="$OUT/lib"
rm -rf "$L"
cp -R mods/lib "$L"
pack "$L" "$OUT/lib.tempest"

for bits in $TARGETS; do
    echo "staged: $OUT/Tempest.Scripting$bits.dll"
done
