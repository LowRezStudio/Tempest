#!/bin/bash
# Round-trip verification: decompile every script package of a cooked game
# directory, recompile each one conformed to its original, and compare the
# rebuilt packages export by export.
#
#   scripts/verify.sh <cooked-dir> [work-dir]
#
# Prints one line per package ("<Pkg>: N diff lines"). Exits non-zero when a
# cooked game package differs; packages that were not cooked (the user's own
# mods built by UDK) are reported but never fail the run, since they were
# compiled against UDK's Engine and legitimately differ in a few places.
set -u
cd "$(dirname "$0")/.."
COOKED=$1
WORK=${2:-target/verify}
U=target/release/uscript
cargo build --release --quiet || exit 1
rm -rf "$WORK"; mkdir -p "$WORK/dumps"
$U project init "$COOKED" "$WORK/proj" >"$WORK/init.log" 2>&1 || { echo "init failed:"; tail -5 "$WORK/init.log"; exit 1; }
$U project build "$WORK/proj" -o "$WORK/out" >"$WORK/build.log" 2>&1 || { echo "build failed:"; grep -v '^warning' "$WORK/build.log" | tail -8; exit 1; }
# Dumps are independent: run them all at once, then compare in manifest order.
while read -r kind name ext cooked _; do
    [ "$kind" = package ] || continue
    $U dump "$COOKED/$name.$ext" "$WORK/dumps/$name.orig" -I "$COOKED" --no-pos --hash >/dev/null 2>&1 &
    $U dump "$WORK/out/$name.$ext" "$WORK/dumps/$name.ours" -I "$WORK/out" -I "$COOKED" --no-pos --hash >/dev/null 2>&1 &
done < "$WORK/proj/uscript-project.txt"
wait
status=0
while read -r kind name ext cooked _; do
    [ "$kind" = package ] || continue
    n=$(diff "$WORK/dumps/$name.orig" "$WORK/dumps/$name.ours" | grep -c '^[<>]')
    note=""
    if [ "$n" != 0 ]; then
        if [ "$cooked" = cooked ]; then status=1; note="  <-- MISMATCH (diff $WORK/dumps/$name.orig $WORK/dumps/$name.ours)"; else note="  (not cooked: UDK-built, differences expected)"; fi
    fi
    echo "$name: $n diff lines$note"
done < "$WORK/proj/uscript-project.txt"
echo "warnings: $(grep -c '^warning' "$WORK/build.log")"
exit $status
