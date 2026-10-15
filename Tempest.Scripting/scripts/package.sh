#!/usr/bin/env bash
# Build Tempest.Scripting64.dll for the Win64 client from macOS/Linux and
# stage an installable test mod:
#
#   dist/Tempest.Scripting64.dll
#   dist/testmod/            manifest.toml + dlls/ + scripts/ + CHECKSUMS
#   dist/testmod.tempest     zip of the above (same format as `tempest mod pack`)
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

STAGE="$OUT/testmod"
rm -rf "$STAGE"
mkdir -p "$STAGE/dlls" "$STAGE/scripts"
cp "$OUT/Tempest.Scripting64.dll" "$STAGE/dlls/"
cp examples/*.lua "$STAGE/scripts/"
cp packaging/manifest.toml "$STAGE/manifest.toml"
# Editor support: API stubs + a LuaLS config pointing at them (and at the
# in-game `tempest.dump_sdk()` output, written to types/sdk).
mkdir -p "$STAGE/types"
cp types/tempest.d.lua "$STAGE/types/"
cat > "$STAGE/.luarc.json" <<'JSON'
{
  "runtime.version": "Lua 5.4",
  "workspace.library": ["types"],
  "workspace.checkThirdParty": false,
  "diagnostics.globals": ["MOD", "shared"]
}
JSON

# CHECKSUMS: "<sha256> <relpath>" lines, ordinal order, LF-joined, no
# trailing newline — byte-for-byte what Tempest.CLI `mod pack` writes.
python3 - "$STAGE" "$OUT/testmod.tempest" <<'EOF'
import hashlib, os, sys, zipfile
stage, outzip = sys.argv[1], sys.argv[2]
files = sorted(
    os.path.relpath(os.path.join(d, f), stage).replace(os.sep, "/")
    for d, _, fs in os.walk(stage) for f in fs
    if f not in ("CHECKSUMS", "CHECKSUMS.asc")
)
lines = []
for rel in files:
    with open(os.path.join(stage, *rel.split("/")), "rb") as fh:
        lines.append(f"{hashlib.sha256(fh.read()).hexdigest()} {rel}")
with open(os.path.join(stage, "CHECKSUMS"), "w", newline="\n") as fh:
    fh.write("\n".join(lines))
if os.path.exists(outzip):
    os.remove(outzip)
with zipfile.ZipFile(outzip, "w", zipfile.ZIP_DEFLATED) as z:
    for d, _, fs in os.walk(stage):
        for f in fs:
            full = os.path.join(d, f)
            z.write(full, os.path.relpath(full, stage).replace(os.sep, "/"))
EOF

# --- second, DLL-less mod exercising the multi-mod runtime ---------------
LIB="$OUT/testmod-lib"
rm -rf "$LIB"
cp -R examples/mods/tempest-scripting-lib "$LIB"
python3 - "$LIB" "$OUT/testmod-lib.tempest" <<'EOF'
import hashlib, os, sys, zipfile
stage, outzip = sys.argv[1], sys.argv[2]
files = sorted(
    os.path.relpath(os.path.join(d, f), stage).replace(os.sep, "/")
    for d, _, fs in os.walk(stage) for f in fs
    if f not in ("CHECKSUMS", "CHECKSUMS.asc")
)
lines = []
for rel in files:
    with open(os.path.join(stage, *rel.split("/")), "rb") as fh:
        lines.append(f"{hashlib.sha256(fh.read()).hexdigest()} {rel}")
with open(os.path.join(stage, "CHECKSUMS"), "w", newline="\n") as fh:
    fh.write("\n".join(lines))
if os.path.exists(outzip):
    os.remove(outzip)
with zipfile.ZipFile(outzip, "w", zipfile.ZIP_DEFLATED) as z:
    for d, _, fs in os.walk(stage):
        for f in fs:
            full = os.path.join(d, f)
            z.write(full, os.path.relpath(full, stage).replace(os.sep, "/"))
EOF

echo "staged: $OUT/Tempest.Scripting64.dll"
echo "packed: $OUT/testmod.tempest + $OUT/testmod-lib.tempest"
