#!/bin/bash
set -e

# Make sure HOME is set to a persistent directory so that Wine prefix, keys, etc. are persisted.
export HOME="/app/data"
export WINEPREFIX="/app/data/prefix"
export WINEDLLOVERRIDES="mscoree,mshtml="
export WINEDEBUG="-all"
mkdir -p "$HOME"

# Start a virtual X framebuffer so Wine can create windows headlessly.
# Without this, Wine fails with "no driver could be loaded" and game server
# processes crash immediately on startup.
export DISPLAY=:99
Xvfb :99 -screen 0 1x1x8 -nolisten tcp -noreset +extension GLX &
XVFB_PID=$!

# Give Xvfb a moment to start
sleep 0.5

# Deploy pre-initialized Wine prefix template if not present in the volume
if [ ! -d "$WINEPREFIX/drive_c" ] && [ -d "/app/wine-prefix-template" ]; then
    echo "=== Deploying pre-initialized Wine prefix template ==="
    mkdir -p "$WINEPREFIX"
    cp -rp /app/wine-prefix-template/. "$WINEPREFIX/"
else
    mkdir -p "$WINEPREFIX"
fi

# Check manifest ID
if [ -n "$TEMPEST_VERSION_ID" ]; then
    MANIFEST_ID="$TEMPEST_VERSION_ID"
elif [ -n "$TEMPEST_VERSION" ]; then
    # Look up ID by version string in versions.json
    if [ -f "/app/versions.json" ]; then
        MANIFEST_ID=$(jq -r --arg ver "$TEMPEST_VERSION" '.[] | select(.version == $ver) | .id' /app/versions.json | head -n 1)
    fi
    if [ -z "$MANIFEST_ID" ] || [ "$MANIFEST_ID" = "null" ]; then
        echo "Error: Version '$TEMPEST_VERSION' not found in versions.json"
        exit 1
    fi
else
    # Fallback to default version if neither is specified
    MANIFEST_ID="4775568787641899396" # Default to 0.57
fi

# 1. Restore the game build
GAME_PATH="${TEMPEST_PATH:-/app/game}"
mkdir -p "$GAME_PATH"

echo "=== Restoring Tempest Game Files (Manifest ID: $MANIFEST_ID) ==="
/app/Tempest.CLI rigby restore "https://tempest-cdn.online/manifests/${MANIFEST_ID}.manifest.json" \
    --out-dir "$GAME_PATH" \
    --base-url "https://tempest-cdn.online/chunks"

# 2. Initialize Wine prefix if it doesn't exist
# We run 'wine init' to set up the wine prefix headlessly.
echo "=== Initializing Wine Prefix ==="
# Prevent interactive wine configuration prompts
export WINEDEBUG=-all
/app/Tempest.CLI wine init

# 3. Build the server open arguments
echo "=== Launching Server ==="
ARGS=()

# Add required path
ARGS+=( "--path" "$GAME_PATH" )

# Name
if [ -n "$TEMPEST_NAME" ]; then
    ARGS+=( "--name" "$TEMPEST_NAME" )
fi

# Tags
if [ -n "$TEMPEST_TAGS" ]; then
    ARGS+=( "--tags" "$TEMPEST_TAGS" )
fi

# Map
if [ -n "$TEMPEST_MAP" ]; then
    ARGS+=( "--map" "$TEMPEST_MAP" )
fi

# Version (default to 0.57 if not specified, or match the selected version string)
if [ -n "$TEMPEST_VERSION" ]; then
    ARGS+=( "--version" "$TEMPEST_VERSION" )
else
    # Try to find version name from ID
    VER_NAME=$(jq -r --arg id "$MANIFEST_ID" '.[] | select(.id == $id) | .version' /app/versions.json | head -n 1)
    if [ -n "$VER_NAME" ] && [ "$VER_NAME" != "null" ]; then
        ARGS+=( "--version" "$VER_NAME" )
    fi
fi

# Max Players
if [ -n "$TEMPEST_MAX_PLAYERS" ]; then
    ARGS+=( "--max-players" "$TEMPEST_MAX_PLAYERS" )
fi

# Min Players
if [ -n "$TEMPEST_MIN_PLAYERS" ]; then
    ARGS+=( "--min-players" "$TEMPEST_MIN_PLAYERS" )
fi

# Flags
if [ "$TEMPEST_JOIN_IN_PROGRESS" = "true" ]; then
    ARGS+=( "--join-in-progress" )
fi

if [ "$TEMPEST_PUBLIC_SERVER" = "true" ]; then
    ARGS+=( "--public-server" )
fi

if [ -n "$TEMPEST_GAMEMODE" ]; then
    ARGS+=( "--gamemode" "$TEMPEST_GAMEMODE" )
fi

if [ -n "$TEMPEST_SERVICES_URL" ]; then
    ARGS+=( "--services-url" "$TEMPEST_SERVICES_URL" )
fi

if [ -n "$TEMPEST_PORT" ]; then
    ARGS+=( "--port" "$TEMPEST_PORT" )
fi

if [ -n "$TEMPEST_GAME_SERVER_PORT" ]; then
    ARGS+=( "--game-server-port" "$TEMPEST_GAME_SERVER_PORT" )
fi

if [ -n "$TEMPEST_PASSWORD" ]; then
    ARGS+=( "--password" "$TEMPEST_PASSWORD" )
fi

if [ "$TEMPEST_NO_DEFAULT_ARGS" = "true" ]; then
    ARGS+=( "--no-default-args" )
fi

if [ -n "$TEMPEST_PLATFORM" ]; then
    ARGS+=( "--platform" "$TEMPEST_PLATFORM" )
fi

if [ -n "$TEMPEST_GAME" ]; then
    ARGS+=( "--game" "$TEMPEST_GAME" )
fi

if [ -n "$TEMPEST_DLL" ]; then
    # TEMPEST_DLL can be comma-separated list
    IFS=',' read -ra DLLS <<< "$TEMPEST_DLL"
    for dll in "${DLLS[@]}"; do
        ARGS+=( "--dll" "$dll" )
    done
fi

if [ "$TEMPEST_ENABLE_JOIN_IN_PROGRESS" = "true" ]; then
    ARGS+=( "--enable-join-in-progress" )
fi

if [ "$TEMPEST_UPNP" = "true" ]; then
    ARGS+=( "--upnp" )
fi

if [ "$TEMPEST_DISCOVER" = "true" ]; then
    ARGS+=( "--discover" )
fi

if [ -n "$TEMPEST_API_KEY" ]; then
    ARGS+=( "--api-key" "$TEMPEST_API_KEY" )
fi

if [ -n "$TEMPEST_COUNTRY" ]; then
    ARGS+=( "--country" "$TEMPEST_COUNTRY" )
fi

# Run the command with evaluated arguments
exec /app/Tempest.CLI server open "${ARGS[@]}"
