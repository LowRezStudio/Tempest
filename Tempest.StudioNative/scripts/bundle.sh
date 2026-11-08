#!/usr/bin/env bash
# Release bundle of the native Studio for the platform it runs on (macOS,
# Linux, Windows under Git Bash), with tempest-cli (the mod cooker's pack,
# sign, install, publish) and upk (tempest-cli rebuilds delta-patched game
# files with it) next to the app:
#
#   macOS    Tempest Studio Native.app (signed, .upk/.twrk associations) as a
#            zip (what the updater downloads) and a DMG
#   Windows  a zip of a folder with tempest-studio.exe
#   Linux    a zip of a folder with tempest-studio and icon.png
#
# Each zip gets a `<zip>.sha256` the updater checks (src/update.rs). Folder
# bundles hold the `.tempest-studio-bundle` marker: a build without it (cargo
# run) never updates itself or registers file associations.
#
# Usage: scripts/bundle.sh [out dir]   (default: Tempest.StudioNative/dist)
#
# Prebuilt parts skip their build: STUDIO_BIN (the app), UPK (upk binary),
# CLI_DIR (a `dotnet publish` folder of Tempest.CLI).
# macOS signing: APPLE_SIGNING_IDENTITY (else ad-hoc); with
# APPLE_NOTARY_PROFILE (a `notarytool store-credentials` profile) the app is
# notarized and stapled too.
set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
repo=$(cd "$here/.." && pwd)
out=${1:-$here/dist}
mkdir -p "$out"
out=$(cd "$out" && pwd)

version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$here/Cargo.toml" | head -1)
case "$(uname -s)" in
	Darwin) os=macos ;;
	MINGW* | MSYS* | CYGWIN*) os=windows ;;
	*) os=linux ;;
esac
case "$(uname -m)" in
	arm64 | aarch64) arch=arm64 ;;
	*) arch=x64 ;;
esac
exe=""
[[ $os == windows ]] && exe=.exe
name="Tempest-Studio-Native-$version-$os-$arch"
echo "Bundling Tempest Studio Native $version for $os-$arch into $out"

if [[ -z ${STUDIO_BIN:-} ]]; then
	"$here/scripts/fetch-ruffle.sh"
	(cd "$here" && cargo build --release)
	STUDIO_BIN=$here/target/release/tempest-studio$exe
fi
if [[ -z ${UPK:-} ]]; then
	(cd "$repo/Tempest.UnrealScript" && cargo build --release -p tempest-assets --bin upk)
	UPK=$repo/Tempest.UnrealScript/target/release/upk$exe
fi
if [[ -z ${CLI_DIR:-} ]]; then
	CLI_DIR=$out/cli-publish
	rm -rf "$CLI_DIR"
	dotnet publish "$repo/Tempest.CLI" -c Release -o "$CLI_DIR"
fi

stage=$out/stage
rm -rf "$stage"
mkdir -p "$stage"

# The binaries every platform ships: the app, upk, tempest-cli and the CLI's native libraries.
put_binaries() {
	local dir=$1
	cp "$STUDIO_BIN" "$dir/tempest-studio$exe"
	cp "$UPK" "$dir/upk$exe"
	cp "$CLI_DIR/Tempest.CLI$exe" "$dir/tempest-cli$exe"
	shopt -s nullglob
	for lib in "$CLI_DIR"/*.dylib "$CLI_DIR"/*.so "$CLI_DIR"/*.dll; do cp "$lib" "$dir/"; done
	shopt -u nullglob
	chmod +x "$dir/tempest-studio$exe" "$dir/upk$exe" "$dir/tempest-cli$exe"
}

checksum() {
	local file=$1
	(cd "$(dirname "$file")" && if command -v sha256sum >/dev/null; then sha256sum "$(basename "$file")"; else shasum -a 256 "$(basename "$file")"; fi) >"$file.sha256"
}

rm -f "$out/$name.zip" "$out/$name.zip.sha256" "$out/$name.dmg"

if [[ $os == macos ]]; then
	app="$stage/Tempest Studio Native.app"
	mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
	put_binaries "$app/Contents/MacOS"
	cp "$here/resources/icon.icns" "$app/Contents/Resources/icon.icns"
	doc_type() {
		cat <<EOF
		<dict>
			<key>CFBundleTypeName</key><string>$2</string>
			<key>CFBundleTypeRole</key><string>Editor</string>
			<key>LSHandlerRank</key><string>Default</string>
			<key>CFBundleTypeExtensions</key><array><string>$1</string></array>
			<key>LSItemContentTypes</key><array><string>com.lowrezstudio.tempest.$1</string></array>
		</dict>
EOF
	}
	uti() {
		cat <<EOF
		<dict>
			<key>UTTypeIdentifier</key><string>com.lowrezstudio.tempest.$1</string>
			<key>UTTypeDescription</key><string>$2</string>
			<key>UTTypeConformsTo</key><array><string>public.data</string></array>
			<key>UTTypeTagSpecification</key><dict><key>public.filename-extension</key><array><string>$1</string></array></dict>
		</dict>
EOF
	}
	cat >"$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key><string>Tempest Studio Native</string>
	<key>CFBundleDisplayName</key><string>Tempest Studio Native</string>
	<key>CFBundleIdentifier</key><string>com.lowrezstudio.tempest.studio-native</string>
	<key>CFBundleExecutable</key><string>tempest-studio</string>
	<key>CFBundleIconFile</key><string>icon</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>CFBundleShortVersionString</key><string>${version%%-*}</string>
	<key>CFBundleVersion</key><string>${version%%-*}</string>
	<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
	<key>LSMinimumSystemVersion</key><string>11.0</string>
	<key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
	<key>NSHighResolutionCapable</key><true/>
	<key>NSSupportsAutomaticGraphicsSwitching</key><true/>
	<key>CFBundleDocumentTypes</key>
	<array>
$(doc_type upk "Unreal Engine 3 package")
$(doc_type twrk "Tempest Studio workspace")
	</array>
	<key>UTImportedTypeDeclarations</key>
	<array>
$(uti upk "Unreal Engine 3 package")
$(uti twrk "Tempest Studio workspace")
	</array>
</dict>
</plist>
EOF
	plutil -lint "$app/Contents/Info.plist" >/dev/null

	identity=${APPLE_SIGNING_IDENTITY:--}
	sign=(codesign --force --sign "$identity" --options runtime)
	[[ $identity != "-" ]] && sign+=(--timestamp)
	# Inside out: the libraries and helpers, then the app (its main executable with it).
	for f in "$app/Contents/MacOS"/*; do
		[[ $(basename "$f") == tempest-studio ]] && continue
		"${sign[@]}" "$f"
	done
	"${sign[@]}" "$app"
	codesign --verify --deep --strict "$app"
	echo "Signed with ${identity/#-/an ad-hoc signature}"

	if [[ -n ${APPLE_NOTARY_PROFILE:-} && $identity != "-" ]]; then
		ditto -c -k --keepParent "$app" "$stage/notarize.zip"
		xcrun notarytool submit "$stage/notarize.zip" --keychain-profile "$APPLE_NOTARY_PROFILE" --wait
		xcrun stapler staple "$app"
		rm "$stage/notarize.zip"
	fi

	ditto -c -k --keepParent "$app" "$out/$name.zip"
	dmg_root=$stage/dmg
	mkdir -p "$dmg_root"
	cp -R "$app" "$dmg_root/"
	ln -s /Applications "$dmg_root/Applications"
	hdiutil create -quiet -volname "Tempest Studio Native" -srcfolder "$dmg_root" -ov -format UDZO "$out/$name.dmg"
	[[ $identity != "-" ]] && codesign --force --sign "$identity" --timestamp "$out/$name.dmg"
else
	dir=$stage/tempest-studio-native
	mkdir -p "$dir"
	put_binaries "$dir"
	touch "$dir/.tempest-studio-bundle"
	[[ $os == linux ]] && cp "$here/resources/icon.png" "$dir/icon.png"
	if [[ $os == windows ]]; then
		# Forward slashes in the entry names (Compress-Archive of Windows PowerShell writes backslashes).
		if command -v 7z >/dev/null; then
			(cd "$stage" && 7z a -tzip -bso0 "$(cygpath -w "$out/$name.zip")" tempest-studio-native)
		else
			(cd "$stage" && /c/Windows/System32/tar.exe -a -c -f "$(cygpath -w "$out/$name.zip")" tempest-studio-native)
		fi
	else
		(cd "$stage" && zip -qry "$out/$name.zip" tempest-studio-native)
	fi
fi

checksum "$out/$name.zip"
rm -rf "$stage"
echo "Done:"
ls -la "$out" | grep -F "$name"
