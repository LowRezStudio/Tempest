#!/usr/bin/env bash
# HOST_CC for cross-building LuaJIT (luajit-src) to i686-pc-windows-gnu from
# a 64-bit host that has no 32-bit host compiler (Apple Silicon macOS).
#
# LuaJIT's build runs two host tools (minilua, buildvm) that must be compiled
# with the *target's* pointer size: buildvm embeds `offsetof` values of VM
# structs into the generated machine code. Instead of `gcc -m32`, compile
# them as 32-bit Windows executables with the target's own MinGW and run
# them under Wine: every `-o host/tool` link produces `host/tool.exe` plus a
# `host/tool` shell script that execs it through `wine`.
#
# Used by scripts/package.sh (HOST_CC=... cargo build --target i686-pc-windows-gnu).
set -euo pipefail

CC="${TEMPEST_HOSTCC32_CC:-i686-w64-mingw32-gcc}"
WINE="${TEMPEST_HOSTCC32_WINE:-wine}"

out=""
compile_only=0
args=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        -c) compile_only=1; args+=("$1"); shift ;;
        -o) out="$2"; shift 2 ;;
        -o*) out="${1#-o}"; shift ;;
        *) args+=("$1"); shift ;;
    esac
done

# Compiles, dependency scans (-MM) and anything without an output: pass through.
if [[ $compile_only -eq 1 || -z "$out" || "$out" == *.o ]]; then
    exec "$CC" "${args[@]}" ${out:+-o "$out"}
fi

# A link: build the .exe, then stand in for the executable with a wrapper.
"$CC" "${args[@]}" -o "$out.exe"
cat > "$out" <<WRAP
#!/usr/bin/env bash
export WINEDEBUG=\${WINEDEBUG:--all}
exec "$WINE" "\$(cd "\$(dirname "\$0")" && pwd)/$(basename "$out").exe" "\$@"
WRAP
chmod +x "$out"
