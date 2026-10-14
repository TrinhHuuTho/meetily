# Contributions in the TriNote fork

TriNote started as a fork of [Meetily](https://github.com/Zackriya-Solutions/meeting-minutes) 0.4.1 by Zackriya Solutions (commit `a2cb62e`, September 2026). This document records what the fork changed, why, and how the results were measured. [CHANGELOG.md](CHANGELOG.md) has the user-facing summary.

## At a glance

| | |
| --- | --- |
| Commits on top of Meetily 0.4.1 | 31 |
| Lines removed / added | −44,361 / +3,127 (excluding lockfiles, fonts and images) |
| Bugs fixed | 6: four lost or damaged recordings, one broke long summaries, one broke the dev build |
| Mixed EN/VI/ZH transcription speed (GTX 1650) | real-time factor 1.24 → 0.125 |
| New capabilities | Mixed-language transcription, PDF export, Linux system audio, GPU auto-selection with crash recovery |

Real-time factor (RTF) is processing time divided by audio length; below 1.0 keeps up with live audio, and lower is faster.

## 1. Removing what the app no longer used

Meetily 0.4.1 still carried an archived Python backend, usage analytics, an auto-updater pointing at the upstream servers, and a lot of unreferenced code. The fork removed it in reviewable steps:

| Commit | Removed | Lines |
| --- | --- | --- |
| `d1bc799` | Archived Python/FastAPI backend, Docker files, whisper-server scripts, uncompiled sources | −30,758 |
| `e3791a3` | PostHog analytics, auto-updater, dead Tauri commands and their UI | −7,137 |
| `cd19466` | Unused frontend components, npm dependencies, the fs plugin | −4,791 |
| `71f5533` | Unused FFmpeg mixer, placeholder modules, dead flags | −540 |
| `63ac909` | Every Rust compiler warning | — |

### Restoring a removed feature

Nothing was commented out, because the code in git history is complete and still compiles at its commit. To bring a feature back, start from the commit that removed it, for example `git show e3791a3` for analytics and the updater. Expect to change more than the files, though. The old updater checked upstream's GitHub releases and verified them with upstream's signing key, and analytics went to PostHog with a key supplied by upstream's build. A restored version needs its own update endpoint, signing key and analytics key, and an updated [PRIVACY_POLICY.md](PRIVACY_POLICY.md).

## 2. Bug fixes

| Commit | Symptom | Cause and fix |
| --- | --- | --- |
| `8d7818a` | Stopped meetings sometimes vanished; one failed chunk made the UI show "stopped" while audio kept recording | Saving depended on a polled status that could fail once and skip the save. Saving is now unconditional, and only errors that really stop transcription end the recording |
| `263f545` | Saved meetings had the wrong duration | The duration was read after the recording state was cleaned up. It is now captured at stop time, excluding pauses |
| `60df178` | After an audio device failure the app kept showing an active recording with no audio, and nothing was saved | The error path stopped capture but never ended the session. Fatal errors now run the normal stop path, which saves the audio and the meeting |
| `23b602d` | Summaries of long Chinese or Vietnamese meetings overflowed the built-in model's context | Tokens were estimated at 0.35 per character for every script. Estimates are now per script (about 1 token per Chinese character), chunks also split at Chinese punctuation, and room is reserved for the output |
| `cdc9267` | On Linux, "system audio" recorded the microphone a second time, so remote participants were missing | ALSA does not list PipeWire monitor sources, and the default device name resolved to an input. System audio is now captured from the default sink monitor with `pw-record` |
| `e005a9c` | `tauri dev` regularly failed with `ChunkLoadError` | A chunk request occasionally hangs in the webview while the dev server is fine. The webview now reloads after a 60 s timeout, in development only |

## 3. Mixed English / Vietnamese / Chinese transcription

**Problem.** Parakeet, the default engine, only handles European languages. Whisper's own auto-detection handles all three languages, but it runs a second full encoder pass on every segment and sometimes picks a neighbouring language (Japanese, Cantonese). On a GTX 1650 it ran at RTF 1.24, slower than live audio.

**Approach** (`cc9bd54`, `5d8ba29`):

- A restricted language mode (`auto:en,vi,zh`): each segment is classified, and only the three allowed languages compete.
- Detection runs on the small `base` model (142 MB) instead of the large model, at roughly a tenth of the cost.
- The encoder context is sized to each segment instead of a padded 30-second window, which cuts encoder work about 4x on short segments.
- Flash attention is enabled on every CUDA and Metal GPU.
- The detected language is stored per segment and shown as EN/VI/ZH badges.

**Result.** The mode ran at RTF 0.14–0.17 with CUDA (whisper.cpp 1.7), about the speed of Parakeet, with the correct language on 40/40 synthetic test segments. After the whisper.cpp 1.8 upgrade (section 6) it runs at 0.125 with Vulkan on a real 341-second mixed meeting.

## 4. PDF export

`4778fea` exports the summary and a timestamped transcript to PDF inside the app with pdfmake. Markdown headings, lists, task items and tables are converted. pdfmake has no font fallback, so text is split into runs by script and Chinese runs use Noto Sans SC. The fonts are bundled (SIL Open Font License) and subset into each file: a 16-page meeting exports to about 90 KB.

## 5. Linux system audio

Recording a call needs both the microphone and what the other side says. On Linux the upstream build silently recorded the microphone twice (see `cdc9267` above). TriNote runs `pw-record` against the default PipeWire sink monitor and feeds it into the same mixing pipeline. Machines without `pw-record` record the microphone only, instead of opening the wrong device.

## 6. GPU auto-selection and CPU fallback

**Problem.** One Linux package has to work on NVIDIA, AMD and Intel GPUs and on machines without a GPU. The Vulkan backend in whisper.cpp 1.7 was 55% slower than CUDA on a GTX 1650 and hung an AMD Renoir integrated GPU: the kernel reset the GPU and the app aborted.

**Approach:**

- whisper.cpp 1.7 → 1.8.3 via whisper-rs 0.16 (`8f4d6f2`). The new Vulkan backend ran the whole benchmark on the same integrated GPU without a single reset, at RTF 0.90 in the EN/VI/ZH mode.
- Device selection (`9508fad`): whisper.cpp 1.8 uses the first GPU it lists, which on hybrid laptops is the integrated one. TriNote now picks the discrete GPU first, then the integrated GPU, then the CPU.
- Crash recovery (`7de64dd`): a GPU fault aborts the process from native code, where Rust cannot catch it. While speech recognition runs on the GPU, a marker file exists; finding it at startup means the last run died on the GPU, so speech recognition switches to the CPU and the user is told. The summary engine runs in its own process, so a crash there is retried on the CPU straight away. A settings switch turns the GPU off or back on.
- Summaries (`ff16c66`, `4e0e8bb`): GPU offload is sized from the free Vulkan memory instead of an assumed 4 GB, and CPU-only mode gives llama.cpp no GPU devices at all. With only zero GPU layers, llama.cpp still sent prompt processing to the GPU.
- The CPU fallback uses every physical core (`a65406d`).

**Results** on a GTX 1650 laptop (Ryzen 5 5500U), EN/VI/ZH mode:

| Build | RTF |
| --- | --- |
| Vulkan, whisper.cpp 1.7 | 0.237 |
| CUDA, whisper.cpp 1.8 | 0.152 |
| **Vulkan, whisper.cpp 1.8 (release build)** | **0.125** |
| Vulkan, whisper.cpp 1.8, AMD Radeon integrated GPU | 0.90 |
| CPU only, 6 cores (was 1.36 with 4 threads) | 1.00 |

Vulkan is now the fastest option even on NVIDIA hardware, so a single Vulkan build serves every GPU vendor. The Linux release does not need a separate CUDA package. More numbers, including summaries and smaller models, are in [docs/GPU_ACCELERATION.md](docs/GPU_ACCELERATION.md).

## 7. Becoming TriNote

- Renamed the app, package and binary (`trinote`), with a new icon and an About page that credits Meetily (`9517e37`, `33d71b6`).
- Own identifier `com.trinhhuutho.trinote`, so TriNote never shares a database, settings or a single-instance lock with an installed Meetily.
- The Parakeet v3 model downloads from HuggingFace, pinned to a commit whose files match the upstream server byte for byte (`224371c`). The fork depends on no upstream infrastructure.

## How the numbers were measured

- Hardware: Acer laptop, AMD Ryzen 5 5500U (6 cores), NVIDIA GeForce GTX 1650 4 GB plus an AMD Radeon integrated GPU, 22 GB RAM, Ubuntu 26.04.
- Speech recognition: the ignored test `bench_language_modes` in `frontend/src-tauri/src/whisper_engine/language_tests.rs` transcribes a folder of WAV segments one by one, like a live recording, and reports RTF per mode. The release numbers use 34 segments (341 s) cut from a real recording that mixes English, Vietnamese and Chinese. The earlier 40/40 language check used synthetic text-to-speech segments.
- Language detection margins: `bench_language_detection_margins` prints the English / Vietnamese / Chinese probabilities per segment on GPU and CPU.
- Summaries: Qwen 3.5 4B summarizing a 4,564-token transcript, timed by `llama-helper`'s own statistics.
