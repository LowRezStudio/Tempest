# Stage 1: build — Zig tools + Native AOT binary
FROM mcr.microsoft.com/dotnet/sdk:10.0 AS build
WORKDIR /src

RUN --mount=type=cache,target=/var/cache/apt \
    --mount=type=cache,target=/var/lib/apt \
    apt-get update && \
    apt-get install -y --no-install-recommends clang zlib1g-dev curl xz-utils && \
    rm -rf /var/lib/apt/lists/*

# Zig 0.15.2 for Tempest.Utils, 0.16.0 for Tempest.CLI/inject.
# Archives live in a cache mount, so base-image updates only re-extract.
RUN --mount=type=cache,target=/var/cache/zig \
    mkdir -p /opt/zig-0.15 /opt/zig-0.16 && \
    [ -f /var/cache/zig/zig-0.15.2.tar.xz ] || \
        curl -fL -o /var/cache/zig/zig-0.15.2.tar.xz https://ziglang.org/download/0.15.2/zig-x86_64-linux-0.15.2.tar.xz && \
    [ -f /var/cache/zig/zig-0.16.0.tar.xz ] || \
        curl -fL -o /var/cache/zig/zig-0.16.0.tar.xz https://ziglang.org/download/0.16.0/zig-x86_64-linux-0.16.0.tar.xz && \
    tar -xJf /var/cache/zig/zig-0.15.2.tar.xz -C /opt/zig-0.15 --strip-components=1 && \
    tar -xJf /var/cache/zig/zig-0.16.0.tar.xz -C /opt/zig-0.16 --strip-components=1

# Project files first, for restore layer caching
COPY Tempest.slnx ./
COPY MarshalLib/MarshalLib.csproj MarshalLib/
COPY MarshalLib.Database/MarshalLib.Database.csproj MarshalLib.Database/
COPY Tempest.Protocol/Tempest.Protocol.csproj Tempest.Protocol/
COPY Tempest.CLI/Tempest.CLI.csproj Tempest.CLI/
COPY Tempest.Utils/build.zig Tempest.Utils/
COPY Tempest.Utils/build.zig.zon Tempest.Utils/

RUN --mount=type=cache,target=/root/.nuget/packages \
    dotnet restore Tempest.CLI/Tempest.CLI.csproj -r linux-x64

COPY MarshalLib/ MarshalLib/
COPY MarshalLib.Database/ MarshalLib.Database/
COPY Tempest.Protocol/ Tempest.Protocol/
COPY Tempest.CLI/ Tempest.CLI/
COPY Tempest.Utils/ Tempest.Utils/

# asmloader DLLs
WORKDIR /src/Tempest.Utils
RUN --mount=type=cache,target=/src/Tempest.Utils/.zig-cache \
    --mount=type=cache,target=/root/.cache/zig \
    PATH="/opt/zig-0.15:${PATH}" zig build -Doptimize=ReleaseSafe

# AOT publish (BuildZigInject compiles inject/ with Zig 0.16.0)
WORKDIR /src
RUN --mount=type=cache,target=/root/.nuget/packages \
    --mount=type=cache,target=/src/Tempest.CLI/inject/.zig-cache \
    --mount=type=cache,target=/root/.cache/zig \
    PATH="/opt/zig-0.16:${PATH}" \
    dotnet publish Tempest.CLI/Tempest.CLI.csproj -c Release -r linux-x64 \
    -o /app/publish --no-restore /p:PublishAot=true

RUN cp /src/Tempest.Utils/zig-out/bin/asmloader-windows_x86.dll \
       /src/Tempest.Utils/zig-out/bin/asmloader-windows_x86_64.dll /app/publish/
COPY Tempest.Launcher/src/lib/data/versions.json /app/publish/

# Stage 2: runtime
FROM debian:trixie-slim
WORKDIR /app

# wine32:i386 required for 0.57, the last Win32-only build
RUN --mount=type=cache,target=/var/cache/apt \
    --mount=type=cache,target=/var/lib/apt \
    dpkg --add-architecture i386 && \
    apt-get update && \
    apt-get install -y --no-install-recommends wine wine32:i386 jq bash ca-certificates libicu76 && \
    rm -rf /var/lib/apt/lists/*

# libpulse in wine aborts without a runtime dir
ENV XDG_RUNTIME_DIR="/tmp/xdg"
RUN mkdir -p /tmp/xdg && chmod 700 /tmp/xdg

COPY --from=build /app/publish/ /app/
COPY Tempest.Launcher/src-tauri/resources/Tempest*Core.tempest /app/mods/
COPY Tempest.Launcher/src-tauri/resources/Tempest*Multiplayer.tempest /app/mods/
COPY --chmod=755 Tempest.CLI/entrypoint.sh /app/entrypoint.sh

EXPOSE 50051/tcp
EXPOSE 7000/udp
VOLUME ["/app/data"]

# TEMPEST_PATH is the games root; the entrypoint appends the version subdir
ENV TEMPEST_PATH="/app/game" \
    TEMPEST_SKIP_RESTORE="false" \
    TEMPEST_GAMEMODE="TempestMp.Siege" \
    TEMPEST_PORT=50051 \
    TEMPEST_GAME_SERVER_PORT=7000 \
    WINEDEBUG="-all"

ENTRYPOINT ["/app/entrypoint.sh"]
