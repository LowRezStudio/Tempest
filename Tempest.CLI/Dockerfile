# =========================================================
# Stage 1: Build Environment
# =========================================================
FROM mcr.microsoft.com/dotnet/sdk:10.0-alpine AS build
WORKDIR /src

# Install required build tools for Native AOT on Alpine, plus gcompat (for protoc), utilities, wine (to pre-initialize the prefix), and xvfb-run (headless X server for wine)
RUN apk add --no-cache clang build-base zlib-dev gcompat xz curl wine freetype libgcc libstdc++ xvfb-run gnutls

# Download and install Zig 0.15.2
RUN mkdir -p /opt/zig && \
    curl -L https://ziglang.org/download/0.15.2/zig-x86_64-linux-0.15.2.tar.xz | tar -xJ -C /opt/zig --strip-components=1
ENV PATH="/opt/zig:${PATH}"

# Copy solution and project files for caching package restore
COPY Tempest.slnx ./
COPY MarshalLib/MarshalLib.csproj MarshalLib/
COPY MarshalLib.Database/MarshalLib.Database.csproj MarshalLib.Database/
COPY Tempest.Protocol/Tempest.Protocol.csproj Tempest.Protocol/
COPY Tempest.CLI/Tempest.CLI.csproj Tempest.CLI/
COPY Tempest.Utils/build.zig Tempest.Utils/
COPY Tempest.Utils/build.zig.zon Tempest.Utils/

# Restore NuGet packages specifically for linux-musl-x64 target using cache mount
RUN --mount=type=cache,target=/root/.nuget/packages \
    dotnet restore Tempest.CLI/Tempest.CLI.csproj -r linux-musl-x64

# Copy source code files
COPY MarshalLib/ MarshalLib/
COPY MarshalLib.Database/ MarshalLib.Database/
COPY Tempest.Protocol/ Tempest.Protocol/
COPY Tempest.CLI/ Tempest.CLI/
COPY Tempest.Utils/ Tempest.Utils/

# Build native utility tools (specifically asmloader dynamic libraries)
WORKDIR /src/Tempest.Utils
RUN --mount=type=cache,target=/src/Tempest.Utils/.zig-cache \
    --mount=type=cache,target=/root/.cache/zig \
    zig build -Doptimize=ReleaseSafe

# Publish Tempest.CLI with Native AOT targeting Alpine using NuGet cache mount
WORKDIR /src
RUN --mount=type=cache,target=/root/.nuget/packages \
    dotnet publish Tempest.CLI/Tempest.CLI.csproj \
    -c Release \
    -r linux-musl-x64 \
    -o /app/publish \
    --no-restore \
    /p:PublishAot=true

# Copy the built asmloader dlls into the published folder
RUN cp /src/Tempest.Utils/zig-out/bin/asmloader-windows_x86.dll /app/publish/ && \
    cp /src/Tempest.Utils/zig-out/bin/asmloader-windows_x86_64.dll /app/publish/

# Copy versions database so version names/IDs can be resolved
COPY Tempest.Launcher/src/lib/data/versions.json /app/publish/

# Pre-initialize Wine prefix template in the build stage
ENV WINEPREFIX="/app/wine-prefix-template"
ENV WINEDLLOVERRIDES="mscoree,mshtml="
ENV WINEDEBUG="-all"
RUN mkdir -p /app/wine-prefix-template && \
    xvfb-run --auto-servernum /app/publish/Tempest.CLI wine init

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
