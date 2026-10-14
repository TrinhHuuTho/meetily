# Building TriNote on Linux

These steps build TriNote from source on Ubuntu (22.04 or newer) and other Debian-based distributions. They were last checked on Ubuntu 26.04. Release packages are built on Ubuntu 22.04 by GitHub Actions so that they run on 22.04 and newer.

## 1. Install system packages

```bash
sudo apt update
sudo apt install -y build-essential curl wget file git pkg-config cmake clang \
  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev libxdo-dev \
  libasound2-dev patchelf \
  libvulkan-dev glslc vulkan-tools \
  pipewire-bin
```

- `libvulkan-dev` and `glslc` build the Vulkan GPU backend, and `vulkan-tools` provides `vulkaninfo` for checking your GPU. Ubuntu 22.04 has no `glslc` package; install the [LunarG Vulkan SDK](https://vulkan.lunarg.com/sdk/home#linux) there, or build for CPU only.
- `pipewire-bin` provides `pw-record`, which TriNote uses at runtime to record your computer's audio.
- For NVIDIA CUDA builds (optional, see below), also install `nvidia-cuda-toolkit`.

## 2. Install Rust, Node.js and pnpm

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # Rust (stable)
# Node.js 20 or newer, from your package manager or https://nodejs.org
npm install -g pnpm@9.15.9
```

## 3. Get the code

```bash
git clone https://github.com/TrinhHuuTho/trinote.git
cd trinote/frontend
pnpm install --frozen-lockfile
```

## 4. Run in development mode

```bash
TAURI_GPU_FEATURE=vulkan ./dev-gpu.sh
```

The script builds the `llama-helper` summary sidecar with the same GPU backend, copies it to `src-tauri/binaries/`, and starts the app with hot reload. The first build compiles whisper.cpp and llama.cpp and downloads a static FFmpeg binary, so it takes several minutes and needs internet access.

After that, `pnpm run tauri:dev:vulkan` starts the app faster by reusing the `llama-helper` already in `src-tauri/binaries/`.

Other backends:

| Backend | Command | Notes |
| --- | --- | --- |
| Vulkan | `TAURI_GPU_FEATURE=vulkan ./dev-gpu.sh` | Same as release builds; NVIDIA, AMD and Intel GPUs |
| CUDA | `TAURI_GPU_FEATURE=cuda ./dev-gpu.sh` | NVIDIA only. The script sets `CMAKE_CUDA_ARCHITECTURES=75` (Turing); change it in `dev-gpu.sh` for other GPU generations |
| CPU only | `pnpm run tauri:dev:cpu` | Uses the existing `llama-helper` in `src-tauri/binaries/` |

Without `TAURI_GPU_FEATURE`, the scripts pick a backend themselves (CUDA when `nvcc` is installed). Release builds always use Vulkan. [GPU_ACCELERATION.md](GPU_ACCELERATION.md) compares the backends.

## 5. Build installable packages

```bash
TAURI_GPU_FEATURE=vulkan ./build-gpu.sh
```

The `.deb` and the AppImage are written to `target/release/bundle/deb/` and `target/release/bundle/appimage/` in the repository root. Running the AppImage needs `libfuse2` (`libfuse2t64` on Ubuntu 24.04 and newer).

Packages built this way are optimized for your own CPU and may not run on older processors. Packages meant for other computers should come from the release workflow, which builds whisper.cpp and llama.cpp without CPU-specific instructions.

## 6. Run the tests

```bash
cd frontend/src-tauri
cargo test --features vulkan --lib
```

The speech recognition benchmarks are ignored tests; the comment on `bench_language_modes` in `src/whisper_engine/language_tests.rs` explains how to run them on your own recordings.

## Troubleshooting

- **`glslc` not found or `Could NOT find Vulkan`.** Install `libvulkan-dev` and `glslc` (step 1), or the LunarG Vulkan SDK on Ubuntu 22.04.
- **Only the microphone is recorded.** Check that `pw-record --version` works and that `pactl info | grep "Server Name"` mentions PipeWire. Ubuntu 22.04 uses PulseAudio by default.
- **`ChunkLoadError` in the development window.** The window reloads by itself after about a minute. To reload sooner, right-click and choose Reload.
- **No GPU acceleration.** See the troubleshooting section of [GPU_ACCELERATION.md](GPU_ACCELERATION.md).
