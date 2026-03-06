#!/bin/sh

APP_CONFIG_DIR="${TAURI_APP_CONFIG_DIR:-}"
if [ -z "$APP_CONFIG_DIR" ]; then
    if [ -n "$XDG_CONFIG_HOME" ]; then
        APP_CONFIG_DIR="$XDG_CONFIG_HOME/com.lowrezstudio.tempest"
    else
        APP_CONFIG_DIR="$HOME/.config/com.lowrezstudio.tempest"
    fi
fi

WINEPREFIX_PATH="$APP_CONFIG_DIR/prefix"

DOTNET_INSTALLER_URL="https://builds.dotnet.microsoft.com/dotnet/Runtime/9.0.6/dotnet-runtime-9.0.6-win-x64.exe"
DOTNET_INSTALLER_NAME="dotnet-installer.exe"

export WINEPREFIX="$WINEPREFIX_PATH"

if [ "$ENABLE_WINE_DEBUG" != "true" ]; then
    export DXVK_LOG_LEVEL=none
    export DXVK_LOG_PATH=/dev/null
    export WINEDEBUG=-all
fi

if [ ! -d "$WINEPREFIX_PATH" ]; then
    echo "--- Wine prefix not found. Creating a new one at: $WINEPREFIX_PATH"

    mkdir -p "$WINEPREFIX_PATH"

    echo "--- Downloading .NET 9.0 (x64) Runtime Installer..."
    curl -s -o "$DOTNET_INSTALLER_NAME" "$DOTNET_INSTALLER_URL"

    if [ $? -ne 0 ]; then
        echo "--- ERROR: Failed to download the .NET installer. Please check the URL and your internet connection."
        exit 1
    fi

    echo "--- Installing .NET 9.0. This may take a few moments..."
    wine "$DOTNET_INSTALLER_NAME" /install /quiet /norestart

    if [ $? -ne 0 ]; then
        echo "--- ERROR: .NET installation failed."
        rm "$DOTNET_INSTALLER_NAME"
        exit 1
    fi

    echo "--- .NET 9.0 installation complete."
    rm "$DOTNET_INSTALLER_NAME"
fi

DOTNET_EXE_PATH="C:\\Program Files\\dotnet\\dotnet.exe"

wine_cmd_args=""
processed_args_for_echo=""

for arg in "$@"; do
    if [ -e "$arg" ]; then
        processed_arg="$(winepath -w "$arg")"
        processed_arg="${processed_arg%\\}"
    else
        processed_arg="$arg"
    fi

    wine_cmd_args="$wine_cmd_args \"$processed_arg\""
    processed_args_for_echo="$processed_args_for_echo \"$processed_arg\""
done

eval "wine \"$DOTNET_EXE_PATH\" Tempest.CLI.dll $wine_cmd_args"

EXIT_CODE=$?

exit $EXIT_CODE
