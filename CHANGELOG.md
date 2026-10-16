# Changelog

All notable changes to TriNote are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow [Semantic Versioning](https://semver.org/).

TriNote is a fork of [Meetily](https://github.com/Zackriya-Solutions/meeting-minutes) 0.4.1. The 1.0.0 entry lists the differences from that release.

## [1.0.0] - 2026-10-16

### Added

- **English / Vietnamese / Chinese meetings.** A new language mode detects each speech segment as English, Vietnamese or Chinese and transcribes it in that language, so mixed-language meetings no longer drift into the wrong language. Transcripts show EN/VI/ZH badges when a meeting mixes languages, and the language is saved with each segment.
- Fast language detection with the small Whisper `base` model, with a one-click download from the language settings.
- **PDF export** of meeting notes: the summary plus a timestamped transcript. Noto Sans and Noto Sans SC are bundled, so Vietnamese diacritics and Chinese characters render without system fonts.
- **Linux system audio** through PipeWire (`pw-record`), so the other participants of a call are recorded, not only your microphone.
- **Automatic GPU selection** for speech recognition: the discrete GPU first, then an integrated GPU, then the CPU. Built-in summaries size their offload to a discrete GPU from its free memory, including on Vulkan, and run on the CPU when there is only an integrated GPU.
- **Use GPU acceleration** switch in Settings → General, showing the GPU in use.
- Recovery from GPU crashes: after a crash on the GPU, speech recognition or summaries switch to the CPU, the user is told, and **Try the GPU again** turns the GPU back on.
- New name, icon and About page.

### Changed

- whisper.cpp 1.7 → 1.8.3 (whisper-rs 0.16). On a GTX 1650 the English / Vietnamese / Chinese mode runs at a real-time factor of 0.125 with Vulkan, faster than the CUDA build (0.152).
- Whisper sizes its encoder context to each segment and uses flash attention on CUDA and Metal, so the multilingual mode runs at roughly Parakeet's speed. Whisper's plain auto-detection ran at RTF 1.24, slower than real time.
- On the CPU, Whisper uses every physical core instead of 4 threads (RTF 1.36 → 1.00 on a 6-core CPU).
- Summaries estimate tokens per script (about 1 token per Chinese character), also split chunks at Chinese sentence punctuation, and reserve room for the output. Long Chinese and Vietnamese transcripts no longer overflow the built-in model's context.
- The Parakeet v3 model downloads from HuggingFace (pinned to a fixed commit) instead of the upstream project's server.
- Own app identifier (`com.trinhhuutho.trinote`), data folder, settings files and recordings folder (`~/Documents/trinote-recordings`), so TriNote can be installed next to Meetily without sharing data.

### Fixed

- Stopping a recording always saves the meeting. A single failed status check used to skip the save without telling the user.
- A single failed transcription chunk no longer stops the recording in the UI while audio keeps being captured in the background.
- Saved meetings keep their real duration, excluding pauses. It used to be replaced by the end time of the last transcript.
- When an audio device error stops capture, the meeting is finalized and saved. The UI used to keep showing an active recording with no audio.
- Linux: choosing system audio opened the microphone a second time, so other participants were never recorded.
- The chunk-loss warning appears again; it listened for an event that is never sent.

### Removed

- Usage analytics (PostHog) and the analytics consent screens.
- The auto-updater and its update dialogs.
- The archived Python/FastAPI backend, Docker setup and whisper-server scripts.
- About 43,000 lines of unused code: dead Tauri commands, unused components and npm dependencies, an unused FFmpeg mixer and placeholder modules.
- Meetily PRO promotions and links to upstream services.

[1.0.0]: https://github.com/TrinhHuuTho/trinote/releases/tag/v1.0.0
