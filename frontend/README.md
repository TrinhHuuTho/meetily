# TriNote app

This folder holds the TriNote desktop app:

- `src/`: Next.js user interface (React, TypeScript, Tailwind)
- `src-tauri/`: Rust core (audio capture, speech recognition, storage, summaries) and Tauri configuration
- `public/`: static assets, including the fonts bundled for PDF export

Common commands, run from this folder:

```bash
pnpm install --frozen-lockfile
TAURI_GPU_FEATURE=vulkan ./dev-gpu.sh     # build llama-helper and run the app in development mode
pnpm run tauri:dev:vulkan                 # run again, reusing the built llama-helper
TAURI_GPU_FEATURE=vulkan ./build-gpu.sh   # build the .deb and AppImage
```

More documentation:

- [Building on Linux](../docs/building_in_linux.md)
- [Architecture](../docs/architecture.md)
- [GPU acceleration](../docs/GPU_ACCELERATION.md)
- [Contributing](../CONTRIBUTING.md)
