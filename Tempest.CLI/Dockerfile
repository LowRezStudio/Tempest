# =========================================================
# Stage 1: Build Environment
# =========================================================
# Pin the SDK patch version for reproducible builds (matches local dev SDK
# 10.0.302). The floating 10.0-alpine tag can move under us at any time.
FROM mcr.microsoft.com/dotnet/sdk:10.0.302-alpine3.24 AS build
WORKDIR /src

# Build tools for the .NET SDK / Grpc.Tools (protoc needs gcompat on musl),
# wine (to pre-initialize the prefix), and xvfb-run (headless X server for wine)
RUN apk add --no-cache clang build-base zlib-dev gcompat xz curl wine freetype libgcc libstdc++ xvfb-run gnutls

# Install anyzig — a zig version manager that reads the `minimum_zig_version`
# from each sub-project's build.zig.zon and invokes the matching zig version.
# This mirrors local dev (homebrew anyzig), so Tempest.CLI/inject resolves zig
# 0.16.0 while other zig projects can pin their own versions.
RUN mkdir -p /opt/anyzig && \
    curl -L -o /tmp/anyzig.tar.gz https://github.com/marler8997/anyzig/releases/download/v2026_03_26/anyzig-x86_64-linux.tar.gz && \
    echo "f9d5a09fbd7c019eecef1a397613ce5baec22872a1c3eb5ab4b1132e917c3d71  /tmp/anyzig.tar.gz" | sha256sum -c - && \
    tar -xz -C /opt/anyzig -f /tmp/anyzig.tar.gz && \
    rm /tmp/anyzig.tar.gz
ENV PATH="/opt/anyzig:${PATH}"

# Copy solution and project files for caching package restore
COPY Tempest.slnx ./
COPY MarshalLib/MarshalLib.csproj MarshalLib/
COPY MarshalLib.Database/MarshalLib.Database.csproj MarshalLib.Database/
COPY Tempest.Protocol/Tempest.Protocol.csproj Tempest.Protocol/
COPY Tempest.CLI/Tempest.CLI.csproj Tempest.CLI/

# Restore NuGet packages specifically for linux-musl-x64 target using cache mount
RUN --mount=type=cache,target=/root/.nuget/packages \
    dotnet restore Tempest.CLI/Tempest.CLI.csproj -r linux-musl-x64

# Copy source code files
COPY MarshalLib/ MarshalLib/
COPY MarshalLib.Database/ MarshalLib.Database/
COPY Tempest.Protocol/ Tempest.Protocol/
COPY Tempest.CLI/ Tempest.CLI/

# Publish Tempest.CLI self-contained for linux-musl-x64.
#
# This is deliberately NOT NativeAOT: ilc's ILScanner dataflow analysis crashes
# on this project under the linux-musl-x64 target (NullReferenceException in
# DataflowAnalyzedMethodNode.HasDynamicDependencies), reproduced with both
# ilcompiler 10.0.10 and 10.0.11. The same project AOT-compiles fine for
# osx-arm64, so this is a RID-specific ilc bug, not a project incompatibility.
# A self-contained (non-AOT) publish sidesteps it entirely; startup speed is
# irrelevant here since the game server boots under Wine anyway.
#
# MSBuild's BuildZigInject target compiles Tempest.CLI/inject (inject32.exe /
# inject64.exe, embedded as resources) with zig during this publish; anyzig
# resolves zig 0.16.0 from inject/build.zig.zon. The /root/.cache/zig mount
# persists anyzig's downloaded zig across rebuilds.
WORKDIR /src
RUN --mount=type=cache,target=/root/.nuget/packages \
    --mount=type=cache,target=/root/.cache/zig \
    dotnet publish Tempest.CLI/Tempest.CLI.csproj \
    -c Release \
    -r linux-musl-x64 \
    --self-contained true \
    -o /app/publish \
    --no-restore \
    /p:PublishAot=false

# Copy versions database so version names/IDs can be resolved
COPY Tempest.Launcher/src/lib/data/versions.json /app/publish/

# Pre-initialize Wine prefix template in the build stage
ENV WINEPREFIX="/app/wine-prefix-template"
ENV WINEDLLOVERRIDES="mscoree,mshtml="
ENV WINEDEBUG="-all"
RUN mkdir -p /app/wine-prefix-template && \
    wine wineboot --init

# =========================================================
# Stage 2: Headless Runner Image
# =========================================================
FROM alpine:3.20
WORKDIR /app

# Install Wine (for running Windows game servers), jq (for parsing JSON), bash (for entrypoint),
# xvfb-run (headless X server so Wine can create windows without a display), gnutls (encryption), and dependencies
RUN apk add --no-cache wine jq bash freetype gcompat libgcc libstdc++ curl icu-libs xvfb-run gnutls

# Copy AOT-compiled binary, native libraries (.so), and resources
COPY --from=build /app/publish/ /app/
COPY --from=build /app/wine-prefix-template/ /app/wine-prefix-template/

# Copy entrypoint script
COPY Tempest.CLI/entrypoint.sh /app/entrypoint.sh
RUN chmod +x /app/entrypoint.sh

# Expose server ports:
# 50051: gRPC/HTTP server port
# 7000: Game server port (UDP)
EXPOSE 50051/tcp
EXPOSE 7000/udp

# Mount volume for persistence of keys, wine prefix configurations, and runtime database
VOLUME ["/app/data"]

# Default environment variables
ENV TEMPEST_PATH="/app/game" \
    TEMPEST_PORT=50051 \
    TEMPEST_GAME_SERVER_PORT=7000 \
    WINEDEBUG="-all"

ENTRYPOINT ["/app/entrypoint.sh"]
