# GPU Acceleration

TriNote runs speech recognition (Whisper) and built-in summaries (llama.cpp) on your GPU when it finds one, and on the CPU otherwise. There is nothing to configure. This page explains how the hardware is chosen, what happens when a GPU misbehaves, how fast each option is, and which models suit which machines.

## How TriNote picks the hardware

| Platform | GPU backend in release builds | Status |
| --- | --- | --- |
| Linux (x86_64) | Vulkan: NVIDIA, AMD and Intel GPUs | v1.0 |
| Windows | Vulkan | planned |
| macOS (Apple Silicon) | Metal | planned |

**Speech recognition (Whisper).** At model load TriNote lists the GPUs the backend can use and picks:

1. the first discrete GPU,
2. otherwise an integrated GPU,
3. otherwise the CPU (no GPU, no Vulkan driver, or GPU acceleration turned off).

Hybrid laptops often list the integrated GPU first, so step 1 matters: on an AMD iGPU + NVIDIA laptop TriNote uses the NVIDIA card. The log shows the decision:

```text
Whisper GPU device 1: Vulkan1 (NVIDIA GeForce GTX 1650, discrete, 4096 MB)
Whisper acceleration decision: compiled_backend=Vulkan gpu=NVIDIA GeForce GTX 1650 use_gpu=true ...
```

When Whisper runs on the CPU it uses every physical core (up to 8); on the GPU it uses 4 CPU threads.

**Summaries (built-in model).** `llama-helper` puts as many model layers on a discrete GPU as fit in its free memory and runs the rest on the CPU. It does not use integrated GPUs: in testing, llama.cpp on an AMD Radeon integrated GPU froze the whole computer, so machines without a discrete GPU summarize on the CPU.

**Parakeet** always runs on the CPU (ONNX Runtime) and is fast enough there.

## Turning the GPU off, and GPU crash recovery

**Settings → General → Use GPU acceleration** switches both engines between "GPU when available" and "CPU only". The card also shows which GPU is in use.

A GPU driver fault can take the whole app down (it happens inside the native inference code, where it cannot be caught). TriNote makes that recoverable:

- **Speech recognition.** While Whisper loads or runs on the GPU, TriNote keeps a marker file (`gpu-transcription.inflight`) in its data folder. If the app starts and finds it, the previous run died on the GPU. Speech recognition then switches to the CPU and a notice explains why.
- **Summaries.** `llama-helper` is a separate process. If it dies while using the GPU, the summary is retried on the CPU and later summaries stay on the CPU.

After updating your graphics driver, use **Try the GPU again** in the same settings card. The choice is stored in `acceleration.json` in the app data folder (`~/.local/share/com.trinhhuutho.trinote/` on Linux).

## Performance

Measured on an Acer laptop: AMD Ryzen 5 5500U (6 cores) with Radeon integrated graphics, NVIDIA GeForce GTX 1650 (4 GB), 22 GB RAM, Ubuntu 26.04, TriNote 1.0 (whisper.cpp 1.8.3).

### Speech recognition

Whisper `large-v3-turbo-q5_0` transcribed 34 real segments (341 s of speech) from a mixed English / Vietnamese / Chinese meeting, one segment at a time like a live recording. RTF (real-time factor) is processing time divided by audio length: lower is faster, and anything below 1.0 keeps up with live audio.

| Backend | Fixed language | English / Vietnamese / Chinese auto |
| --- | --- | --- |
| Vulkan on GTX 1650 (release build) | 0.111 | **0.125** |
| CUDA on GTX 1650 (built from source) | 0.125 | 0.152 |
| Vulkan on the Radeon integrated GPU | 0.83 | 0.90 |
| CPU only, 6 cores | 0.82 | 1.00 |

The CPU rows come from 5 of the segments (41 s). The auto mode adds a language check per segment with the small `base` model; without `base` downloaded, the large model does the check itself and is slower.

Smaller Whisper models on the same CPU (EN/VI/ZH auto mode):

| Model | Size | RTF | Notes |
| --- | --- | --- | --- |
| `large-v3-turbo-q5_0` | 547 MB | 1.00 | Best accuracy, barely real time on CPU |
| `small-q5_1` | 181 MB | 0.37 | Good English and Chinese, Vietnamese mostly right |
| `base` | 142 MB | 0.27 | Fine for English, weak for Vietnamese |

### Summaries

Qwen 3.5 4B (Q4_K_M) summarizing a 4,564-token transcript into 512 tokens:

| Backend | Prompt processing | Generation | Total |
| --- | --- | --- | --- |
| Vulkan, GTX 1650 (10 of 33 layers on the GPU) | 235 tokens/s | 7.1 tokens/s | 91 s |
| CUDA, GTX 1650 (9 of 33 layers on the GPU) | 137 tokens/s | 8.9 tokens/s | 91 s |
| CPU only | 38 tokens/s | 6.2 tokens/s | 205 s |

With 4 GB of VRAM most of the model stays on the CPU, so a bigger GPU helps summaries far more than it does here.

## Choosing models for your machine

| Your machine | Speech recognition | Summaries |
| --- | --- | --- |
| Discrete GPU with 4 GB+ (NVIDIA, AMD) | Whisper `large-v3-turbo-q5_0` plus `base` for EN/VI/ZH meetings; Parakeet for English-only | Built-in Qwen 3.5 4B |
| Integrated GPU or CPU only, 6+ cores | Parakeet for English and European languages; Whisper `small-q5_1` for Vietnamese or Chinese | Built-in Qwen 3.5 2B, or a cloud provider |
| Older or 4-core CPU | Parakeet; Whisper `base` for Vietnamese or Chinese | A cloud provider (OpenAI, Anthropic, Groq, OpenRouter) or Ollama on another computer |

Integrated GPUs vary a lot. Try GPU acceleration first, and switch to CPU only in Settings if transcription falls behind or the app becomes unstable.

## Building with another backend

Release builds use Vulkan on Linux. To build from source with a specific backend (see [building_in_linux.md](building_in_linux.md) for setup):

```bash
cd frontend
pnpm run tauri:dev:vulkan    # Vulkan (needs libvulkan-dev and glslc)
pnpm run tauri:dev:cuda      # NVIDIA CUDA (needs the CUDA toolkit)
pnpm run tauri:dev:cpu       # CPU only
```

Use `tauri:build:<backend>` for a release build. Build `llama-helper` with the matching feature, for example `cargo build --release -p llama-helper --features vulkan`.

The benchmarks above come from ignored tests in `frontend/src-tauri/src/whisper_engine/language_tests.rs`; the comment on `bench_language_modes` shows how to run them.

## Troubleshooting

- **No GPU found.** Check that Vulkan sees your GPU with `vulkaninfo --summary` (package `vulkan-tools`). NVIDIA's proprietary driver includes Vulkan support; AMD and Intel GPUs need `mesa-vulkan-drivers`.
- **The wrong GPU is used.** Look for the `Whisper GPU device` lines in the log, and please open an issue with them.
- **Transcription falls behind.** Pick a smaller model (see the table above), or check that GPU acceleration is on and a discrete GPU is listed in Settings.
