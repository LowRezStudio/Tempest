#!/usr/bin/env bash
# Builds Ruffle's self-hosted player from the release in RUFFLE_TAG with the
# patches in ruffle/*.patch applied, into vendor/ruffle/ (gitignored). Vite uses
# vendor/ruffle when it exists, else the stock @ruffle-rs/ruffle from npm.
#
# Patches:
#   0001-scale9grid.patch  ruffle-rs/ruffle#23681 (DisplayObject.scale9Grid and
#                          DefineScalingGrid, 9-slice scaling), rebased on v0.7.1,
#                          without its tests
#
# Needs git, Rust (rustup), Node >= 22 and npm. wasm-bindgen-cli (the version
# Ruffle's Cargo.lock pins) and binaryen's wasm-opt are installed into the work
# folder when missing. RUFFLE_SRC overrides the work folder (default .ruffle/).
# Skips the build when vendor/ruffle was built from the same tag and patches.
set -euo pipefail

RUFFLE_TAG=v0.7.1
BINARYEN_VERSION=133

studio=$(cd "$(dirname "$0")/.." && pwd)
src=${RUFFLE_SRC:-$studio/.ruffle}
out=$studio/vendor/ruffle
tools=$src/.tools

stamp=$( (echo "$RUFFLE_TAG"; cat "$studio"/ruffle/*.patch) | sha256sum | cut -d' ' -f1)
if [[ -f $out/ruffle.js && "$(cat "$out/.stamp" 2>/dev/null)" == "$stamp" ]]; then
	echo "vendor/ruffle is up to date ($RUFFLE_TAG + patches)"
	exit 0
fi

if [[ ! -d $src/.git ]]; then
	git clone --depth 1 --branch "$RUFFLE_TAG" https://github.com/ruffle-rs/ruffle.git "$src"
else
	git -C "$src" fetch --depth 1 origin tag "$RUFFLE_TAG"
	git -C "$src" checkout -q -f "$RUFFLE_TAG"
	git -C "$src" clean -q -fd -e .tools -e web/node_modules -e target
fi
for patch in "$studio"/ruffle/*.patch; do
	echo "applying $(basename "$patch")"
	git -C "$src" apply "$patch"
done

mkdir -p "$tools/bin"
export PATH=$tools/bin:$PATH

command -v rustup >/dev/null && rustup target add wasm32-unknown-unknown

bindgen=$(grep -A1 '^name = "wasm-bindgen"$' "$src/Cargo.lock" | sed -n 's/^version = "\(.*\)"$/\1/p')
if [[ "$(wasm-bindgen --version 2>/dev/null)" != "wasm-bindgen $bindgen" ]]; then
	cargo install wasm-bindgen-cli --version "$bindgen" --locked --root "$tools"
fi

# Without wasm-opt Ruffle still builds, with a slower module.
if ! command -v wasm-opt >/dev/null; then
	case "$(uname -s)-$(uname -m)" in
		Linux-x86_64) asset=x86_64-linux ;;
		Linux-aarch64) asset=aarch64-linux ;;
		Darwin-x86_64) asset=x86_64-macos ;;
		Darwin-arm64) asset=arm64-macos ;;
		*) asset= ;;
	esac
	if [[ -n $asset ]]; then
		curl -fsSL "https://github.com/WebAssembly/binaryen/releases/download/version_$BINARYEN_VERSION/binaryen-version_$BINARYEN_VERSION-$asset.tar.gz" |
			tar xz -C "$tools"
		export PATH=$tools/binaryen-version_$BINARYEN_VERSION/bin:$PATH
	fi
fi

cd "$src/web"
npm ci
npm run build --workspace=ruffle-core
npm run build --workspace=ruffle-selfhosted

rm -rf "$out"
mkdir -p "$out"
cp -r packages/selfhosted/dist/. "$out/"
echo "$stamp" >"$out/.stamp"
echo "built vendor/ruffle ($RUFFLE_TAG + $(ls "$studio"/ruffle/*.patch | wc -l) patches)"
