# Building from source

Below are instructions on how to build Tempest from source.

## Cloning the repository

```bash
git clone https://github.com/LowRezStudio/Tempest.git
```

> **Windows**: You should add the Tempest directory to your Windows Defender exclusions to prevent it from interfering with the build process and speed up development.

## Prerequisites
- [Node.js](https://nodejs.org/en/download/) (ideally version 24 or higher)
- [Zig](https://ziglang.org/download/) (version 0.15.2)
- [Rust](https://www.rust-lang.org/tools/install)
- [.NET SDK](https://dotnet.microsoft.com/download) (version 10.0)

### Windows
- [Visual Studio 2022](https://visualstudio.microsoft.com/vs/) with the "Desktop development with C++" workload installed.

### Linux
- [Tauri prerequisites](https://tauri.app/start/prerequisites/#linux)

### macOS
- [Tauri prerequisites](https://tauri.app/start/prerequisites/#macos)

> If you're **not** planning to work on the launcher, you can skip installing Rust, Node.js and the tauri prerequisites.

## Building the project

If you build one part of the project, the other parts will automatically be built as well, so you can start with any of the three parts.

### Building the launcher

```bash
cd Tempest.Launcher/
# Enable corepack, if it's missing run 'npm i -g corepack' and then run 'corepack enable', additionally follow https://github.com/nodejs/corepack?tab=readme-ov-file#-corepack if required
corepack enable
pnpm install

# If you want to work on the launcher in development mode
pnpm tauri dev

# If you want to build the production version
pnpm tauri build
# The build artifacts will be located in `Tempest.Launcher/src-tauri/target/release/`
```

### Building the CLI

```bash
cd Tempest.CLI/

# If you want to just run a cli command
dotnet run -- --help

# If you want to have a production executable of the launcher
dotnet publish -c Release
# The build artifacts will be located in `Tempest.CLI/bin/Release/net10.0/publish/`
```

### Building the utils

```bash
cd Tempest.Utils/

zig build -Doptimize=ReleaseSafe
# The build artifacts will be located in `Tempest.Utils/zig-out/bin/`
```

# Code Editor setup

We recommend using [Visual Studio Code](https://code.visualstudio.com/) with the following extensions:
- [C#](https://marketplace.visualstudio.com/items?itemName=ms-dotnettools.csharp) for the CLI
- [Zig](https://marketplace.visualstudio.com/items?itemName=ziglang.vscode-zig) if you plan on working on the utils
- [Svelte for VS Code](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) if you plan on working on the launcher
- [Tailwind CSS IntelliSense](https://marketplace.visualstudio.com/items?itemName=bradlc.vscode-tailwindcss) if you plan on working on the launcher

# Techstack

- Launcher: [Tauri](https://tauri.app/)
    - Frontend: [Svelte](https://svelte.dev/)
    - Styling: [DaisyUI](https://daisyui.com/) with [TailwindCSS](https://tailwindcss.com/)
    - A lot of the core logic is implemented in .NET through the CLI
- CLI: [.NET](https://dotnet.microsoft.com/)
- Utils: [Zig](https://ziglang.org/)
