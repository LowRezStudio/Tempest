#!/bin/bash
set -e

export HOME="/app/data"
export WINEPREFIX="/app/data/prefix"
export WINEDLLOVERRIDES="mscoree,mshtml="
export WINEDEBUG="-all"
mkdir -p "$HOME"

# Headless wine: with DISPLAY unset it uses its null/tty driver, no X needed
unset DISPLAY

# Resolve manifest ID
if [ -n "$TEMPEST_VERSION_ID" ]; then
    MANIFEST_ID="$TEMPEST_VERSION_ID"
elif [ -n "$TEMPEST_VERSION" ]; then
    MANIFEST_ID=$(jq -r --arg ver "$TEMPEST_VERSION" '.[] | select(.version == $ver) | .id' /app/versions.json | head -n 1)
    if [ -z "$MANIFEST_ID" ] || [ "$MANIFEST_ID" = "null" ]; then
        echo "Error: Version '$TEMPEST_VERSION' not found in versions.json"
        exit 1
    fi
else
    MANIFEST_ID="4775568787641899396" # 0.57
fi

# Version name: used for the game subdir and --version
if [ -n "$TEMPEST_VERSION" ]; then
    VERSION_NAME="$TEMPEST_VERSION"
else
    VERSION_NAME=$(jq -r --arg id "$MANIFEST_ID" '.[] | select(.id == $id) | .version' /app/versions.json | head -n 1)
    if [ -z "$VERSION_NAME" ] || [ "$VERSION_NAME" = "null" ]; then
        VERSION_NAME="0.57"
    fi
fi

GAME_PATH="${TEMPEST_PATH:-/app/game}/$VERSION_NAME"
mkdir -p "$GAME_PATH"

# Restore (skip when mounting a pre-provisioned game dir)
if [ "$TEMPEST_SKIP_RESTORE" != "true" ]; then
    echo "=== Restoring Tempest Game Files (Manifest ID: $MANIFEST_ID) ==="
    /app/Tempest.CLI rigby restore "https://tempest-cdn.online/manifests/${MANIFEST_ID}.manifest.json" \
        --out-dir "$GAME_PATH" \
        --base-url "https://tempest-cdn.online/chunks"
fi

# Tempest mods. Must run after restore: restore would delete mod files.
install_mod() {
    if [ -f "$1" ]; then
        echo "=== Installing $(basename "$1") ==="
        /app/Tempest.CLI mod install "$GAME_PATH" "$1" --replace --allow-unsigned
    fi
}
if [ "$VERSION_NAME" = "0.56" ] || [ "$VERSION_NAME" = "0.57" ]; then
    install_mod "/app/mods/Tempest Core.tempest"
fi
install_mod "/app/mods/Tempest Multiplayer.tempest"

# Wine prefix (auto-created on first run)
echo "=== Initializing Wine Prefix ==="
/app/Tempest.CLI wine init

# Server args
ARGS=( "--path" "$GAME_PATH" "--version" "$VERSION_NAME" "--gamemode" "${TEMPEST_GAMEMODE:-TempestMp.Siege}" )

if [ -n "$TEMPEST_NAME" ]; then ARGS+=( "--name" "$TEMPEST_NAME" ); fi
if [ -n "$TEMPEST_TAGS" ]; then ARGS+=( "--tags" "$TEMPEST_TAGS" ); fi
if [ -n "$TEMPEST_MAP" ]; then ARGS+=( "--map" "$TEMPEST_MAP" ); fi
if [ -n "$TEMPEST_MAX_PLAYERS" ]; then ARGS+=( "--max-players" "$TEMPEST_MAX_PLAYERS" ); fi
if [ -n "$TEMPEST_MIN_PLAYERS" ]; then ARGS+=( "--min-players" "$TEMPEST_MIN_PLAYERS" ); fi
if [ "$TEMPEST_JOIN_IN_PROGRESS" = "true" ]; then ARGS+=( "--join-in-progress" ); fi
if [ "$TEMPEST_PUBLIC_SERVER" = "true" ]; then ARGS+=( "--public-server" ); fi
if [ -n "$TEMPEST_SERVICES_URL" ]; then ARGS+=( "--services-url" "$TEMPEST_SERVICES_URL" ); fi
if [ -n "$TEMPEST_PORT" ]; then ARGS+=( "--port" "$TEMPEST_PORT" ); fi
if [ -n "$TEMPEST_GAME_SERVER_PORT" ]; then ARGS+=( "--game-server-port" "$TEMPEST_GAME_SERVER_PORT" ); fi
if [ -n "$TEMPEST_PASSWORD" ]; then ARGS+=( "--password" "$TEMPEST_PASSWORD" ); fi
if [ "$TEMPEST_NO_DEFAULT_ARGS" = "true" ]; then ARGS+=( "--no-default-args" ); fi
if [ -n "$TEMPEST_PLATFORM" ]; then ARGS+=( "--platform" "$TEMPEST_PLATFORM" ); fi
if [ -n "$TEMPEST_GAME" ]; then ARGS+=( "--game" "$TEMPEST_GAME" ); fi
if [ -n "$TEMPEST_DLL" ]; then
    IFS=',' read -ra DLLS <<< "$TEMPEST_DLL"
    for dll in "${DLLS[@]}"; do ARGS+=( "--dll" "$dll" ); done
fi
if [ "$TEMPEST_ENABLE_JOIN_IN_PROGRESS" = "true" ]; then ARGS+=( "--enable-join-in-progress" ); fi
if [ "$TEMPEST_UPNP" = "true" ]; then ARGS+=( "--upnp" ); fi
if [ "$TEMPEST_DISCOVER" = "true" ]; then ARGS+=( "--discover" ); fi
if [ -n "$TEMPEST_API_KEY" ]; then ARGS+=( "--api-key" "$TEMPEST_API_KEY" ); fi
if [ -n "$TEMPEST_COUNTRY" ]; then ARGS+=( "--country" "$TEMPEST_COUNTRY" ); fi

exec /app/Tempest.CLI server open "${ARGS[@]}"
