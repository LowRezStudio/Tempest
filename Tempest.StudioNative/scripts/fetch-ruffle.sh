#!/usr/bin/env bash
# Ruffle's Rust sources for the movie player (`src/swf_player.rs`): the release
# in RUFFLE_TAG with the Electron Studio's patches (Tempest.Studio/ruffle/*.patch,
# 9-slice scaling) applied, into vendor/ruffle/ (gitignored), which Cargo.toml
# uses as path dependencies. Run once after cloning, and again when the tag or a
# patch changes (`just studio-native` and scripts/bundle.sh run it).
set -euo pipefail

RUFFLE_TAG=v0.7.1

here=$(cd "$(dirname "$0")/.." && pwd)
patches=$here/../Tempest.Studio/ruffle
out=$here/vendor/ruffle
sha256() { if command -v sha256sum >/dev/null; then sha256sum; else shasum -a 256; fi; }
stamp=$( (echo "$RUFFLE_TAG"; cat "$patches"/*.patch) | sha256 | cut -d' ' -f1)
if [[ -f $out/core/Cargo.toml && "$(cat "$out/.tempest-stamp" 2>/dev/null)" == "$stamp" ]]; then
	exit 0
fi
if [[ ! -d $out/.git ]]; then
	rm -rf "$out"
	git -c advice.detachedHead=false clone -q --depth 1 --branch "$RUFFLE_TAG" https://github.com/ruffle-rs/ruffle.git "$out"
else
	git -C "$out" fetch -q --depth 1 origin tag "$RUFFLE_TAG"
	git -c advice.detachedHead=false -C "$out" checkout -q -f "$RUFFLE_TAG"
	git -C "$out" clean -q -fd
fi
for patch in "$patches"/*.patch; do
	git -C "$out" apply "$patch"
done
echo "$stamp" >"$out/.tempest-stamp"
echo "vendor/ruffle: Ruffle $RUFFLE_TAG + $(ls "$patches"/*.patch | wc -l | tr -d ' ') patch(es)"
