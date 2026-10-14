# Building TriNote from Source

| Platform | Status | Guide |
| --- | --- | --- |
| Linux | Supported, release packages since 1.0 | [building_in_linux.md](building_in_linux.md) |
| Windows | Planned | Notes below |
| macOS (Apple Silicon) | Planned | Notes below |

TriNote does not ship Windows or macOS builds yet. The notes below come from Meetily, which TriNote is forked from, and have not been verified with TriNote. Expect to adjust them.

## Common steps

Install the committed frontend dependencies before building:

```bash
npm install -g pnpm@9.15.9
cd frontend
pnpm install --frozen-lockfile
```

A frozen install keeps the installed packages identical to `pnpm-lock.yaml`. When you change dependencies on purpose, commit the updated lockfile.

## Windows (not yet supported by TriNote)

### Dependencies

- **Node.js** from [nodejs.org](https://nodejs.org/)
- **Rust** from [rust-lang.org](https://www.rust-lang.org/tools/install)
- **pnpm** 9.15.9: `npm install -g pnpm@9.15.9`
- **Visual Studio Build Tools** with the "Desktop development with C++" workload
- **CMake** from [cmake.org](https://cmake.org/download/)
- **Vulkan SDK** from [LunarG](https://vulkan.lunarg.com/) for GPU builds

### Build and run

```powershell
pnpm install --frozen-lockfile
pnpm run tauri:dev:vulkan     # development mode with hot reload
pnpm run tauri:build:vulkan   # production build
```

### Builds for other computers

A local build is tuned to the CPU it was built on. Meetily's distribution workflows set `CMAKE_PROJECT_INCLUDE` to [`.github/force-portable-ggml.cmake`](../.github/force-portable-ggml.cmake), which forces `GGML_NATIVE=OFF` for whisper.cpp, and build Rust with `RUSTFLAGS=-C target-cpu=x86-64-v2`. The resulting installer needs an AVX2-capable x64 CPU, but not AVX-512.

## macOS (not yet supported by TriNote)

Metal GPU acceleration is enabled automatically on macOS.

```bash
# Homebrew: https://brew.sh
brew install cmake node pnpm

pnpm run tauri:dev     # development mode with hot reload
pnpm run tauri:build   # production build
```

Meetily only supported Apple Silicon (M1 and newer); TriNote plans the same.
