# CLAUDE.md

This file guides Claude Code (claude.ai/code) and other contributors working in this repository.

## Project overview

**TriNote** is a privacy-first desktop app that records, transcribes and summarizes meetings on the user's own computer, with a focus on meetings that mix English, Vietnamese and Chinese. It is a fork of Meetily 0.4.1 by Zackriya Solutions (MIT); see `CONTRIBUTIONS.md` for what the fork changed.

- **App:** Tauri 2 (Rust core) + Next.js 14 / React 18 UI, all under `frontend/`
- **Speech recognition:** Parakeet (ONNX Runtime, CPU) and Whisper (whisper.cpp 1.8 via whisper-rs 0.16)
- **Summaries:** the `llama-helper` sidecar (llama.cpp, in `llama-helper/`), Ollama, or cloud providers (OpenAI, Anthropic, Groq, OpenRouter, custom OpenAI-compatible servers)
- **Storage:** SQLite through sqlx, with migrations in `frontend/src-tauri/migrations/`
- **Released platforms:** Linux x86_64 (Vulkan build) since 1.0; Windows and macOS (Apple Silicon) are planned

There is no separate backend server. Everything goes through Tauri commands and events.

## Essential commands

Run from `frontend/`:

```bash
pnpm install --frozen-lockfile
TAURI_GPU_FEATURE=vulkan ./dev-gpu.sh     # builds llama-helper (Vulkan), copies it to src-tauri/binaries/, runs tauri dev
pnpm run tauri:dev:vulkan                 # tauri dev reusing the llama-helper already in src-tauri/binaries/
pnpm run tauri:dev:cuda                   # NVIDIA CUDA instead of Vulkan
pnpm run tauri:dev:cpu                    # CPU only
TAURI_GPU_FEATURE=vulkan ./build-gpu.sh   # release build: .deb and AppImage in ../target/release/bundle/
```

Checks:

```bash
./node_modules/.bin/tsc --noEmit                     # frontend types
cd src-tauri && cargo test --features vulkan --lib   # Rust unit tests
```

The Next.js dev server listens on <http://localhost:3118>. Do not run `next build` while `tauri dev` is running: both write to `frontend/.next` and the dev window breaks.

Benchmarks are ignored tests in `src-tauri/src/whisper_engine/language_tests.rs` (`bench_language_modes`, `bench_language_detection_margins`); their doc comments show the environment variables to set.

## Architecture

See `docs/architecture.md` for diagrams. Key paths:

```text
frontend/src-tauri/src/
├── lib.rs                     # Tauri setup, command registration
├── hardware_acceleration.rs   # GPU/CPU preference, GPU crash marker and fallback
├── audio/
│   ├── capture/               # microphone.rs (cpal), pipewire.rs (Linux system audio via pw-record),
│   │                          # core_audio.rs (macOS process tap), system.rs
│   ├── devices/               # device discovery; platform/{linux,macos,windows}.rs
│   ├── pipeline.rs            # resample to 48 kHz, mix mic+system in 600 ms windows (no ducking), VAD
│   ├── vad.rs                 # Silero VAD at 16 kHz
│   ├── incremental_saver.rs   # MP4 checkpoints every 30 s, joined with FFmpeg on stop
│   ├── transcription/         # single worker: segments → engine → transcript-update events
│   ├── recording_manager.rs   # recording orchestration
│   └── recording_commands.rs  # start/stop/pause commands
├── whisper_engine/
│   ├── whisper_engine.rs      # model loading, LanguageMode (auto, auto-translate, fixed, auto:en,vi,zh), transcription
│   └── acceleration.rs        # GPU device list and selection: discrete → integrated → CPU
├── parakeet_engine/           # Parakeet ONNX engine and model downloads
├── summary/                   # prompts, chunking (per-script token estimates), providers
│   └── summary_engine/        # llama-helper sidecar manager (sidecar.rs) and client (client.rs)
├── database/                  # repositories and setup
└── export.rs                  # save_pdf_file for PDF export
```

Frontend (`frontend/src/`):

- `app/` contains the pages: main recording view, `meeting-details`, `settings`.
- `components/` contains the UI; `contexts/` and `components/Sidebar/SidebarProvider.tsx` hold global state.
- `lib/pdf/meetingPdf.ts` builds PDFs in the webview with pdfmake and the Noto fonts in `public/fonts/`.
- `constants/languages.ts` is the single language list, including `auto:en,vi,zh`.

### Rust ↔ UI communication

Commands (UI → Rust):

```rust
#[tauri::command]
async fn my_command(arg: String) -> Result<String, String> { /* ... */ }
// register in lib.rs: .invoke_handler(tauri::generate_handler![..., my_command])
```

```typescript
const result = await invoke<string>('my_command', { arg: 'value' });
```

Events (Rust → UI): `app.emit("transcript-update", payload)` and `listen<T>('transcript-update', handler)` on the UI side. The usual flow is: a command changes Rust state → Rust emits an event → listeners update React state.

## Important behavior and gotchas

- **App identifier** is `com.trinhhuutho.trinote`. Linux data lives in `~/.local/share/com.trinhhuutho.trinote/` (database, `models/`, settings JSON, webview storage). Recordings default to `~/Documents/trinote-recordings/`. Never write to an installed Meetily's folders (`com.meetily.ai`).
- **Audio sample rates:** capture streams are resampled to 48 kHz; VAD and the speech engines use 16 kHz.
- **System audio on Linux** needs `pw-record` (PipeWire). Without it, only the microphone is recorded. Other ALSA output names are rejected rather than opened as inputs.
- **GPU:** release builds use Vulkan. `acceleration.rs` prefers discrete GPUs because whisper.cpp would otherwise take the first listed device, often an integrated GPU. GPU work in Whisper is wrapped in `hardware_acceleration::transcription_gpu_scope()`, which keeps a crash marker on disk. `llama-helper` runs on the CPU when `LLAMA_CPU_ONLY` is set (empty device list, no op offload).
- **Language detection:** in `auto:en,vi,zh`, a small model (`base`, `tiny`, `small`) classifies each segment. Without one, the main model does it, which is much slower.
- **Models load once** and stay cached; changing the acceleration setting reloads the Whisper model unless a recording is running.
- **Privacy:** no analytics, telemetry or update checks. Do not add network calls the user did not ask for; anything that sends data off the computer must be opt-in and documented in `PRIVACY_POLICY.md`.
- **Hot-path logging:** use `perf_debug!()` (defined in `lib.rs`, compiled out of release builds) in audio and transcription loops.
- **Paths:** use Tauri's path APIs (`app_data_dir`, etc.), never hard-coded paths.

## Conventions

- Rust errors use `anyhow::Result` internally and `Result<_, String>` at the command boundary. The UI catches errors and shows friendly messages with `sonner` toasts.
- Audio devices are called "microphone" and "system", not "input" and "output".
- Branches: `main` (releases), `release/<version>`, `fix/*`, `enhance/*`.
- Commit messages follow Conventional Commits (`feat(scope): ...`, `fix(...)`, `perf(...)`, `docs:`, `test:`, `chore:`).
- When a change makes a document outdated (README, `docs/`, CHANGELOG, privacy policy), update the document in the same change.
