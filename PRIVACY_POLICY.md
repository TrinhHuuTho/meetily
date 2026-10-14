# TriNote Privacy Policy

Last updated: 14 October 2026

TriNote records, transcribes and summarizes meetings on your own computer. It has no accounts, no analytics, no crash reporting and no automatic update checks. Your audio and transcripts leave your computer only if you choose a cloud AI provider for summaries.

## What TriNote stores, and where

Everything stays on your computer. On Linux the locations are:

| Data | Location |
| --- | --- |
| Recordings (`audio.mp4`, `transcripts.json`, `metadata.json` per meeting) | `~/Documents/trinote-recordings/` by default |
| Meetings, transcripts, summaries and settings, including any API keys you enter | `~/.local/share/com.trinhhuutho.trinote/meeting_minutes.sqlite` |
| Downloaded speech and summary models | `~/.local/share/com.trinhhuutho.trinote/models/` |
| Other app settings, such as GPU acceleration | `~/.local/share/com.trinhhuutho.trinote/` |
| Notification settings | `~/.config/trinote/` |
| Custom summary templates | `~/.local/share/TriNote/templates/` |

API keys are stored unencrypted in the local database, so anyone who can read your home folder can read them. Use keys you can revoke.

## When TriNote connects to the internet

1. **Downloading models.** Speech recognition and built-in summary models are downloaded from [Hugging Face](https://huggingface.co) when you download them in onboarding or Settings. Hugging Face sees your IP address and which files you download. No meeting data is sent.
2. **Cloud AI summaries, only if you choose one.** If you select OpenAI, Anthropic (Claude), Groq, OpenRouter, a custom OpenAI-compatible server, or an Ollama server on another computer, the transcript text and the summary instructions are sent to that service whenever you generate a summary. That provider's privacy policy then applies to what you send. The **Built-in AI** option and an Ollama server on your own computer keep everything local.
3. **Links you click.** Links such as the source code or the Ollama download page open in your web browser.

Speech recognition (Parakeet and Whisper) always runs on your computer. Audio is never uploaded.

## Recording other people

TriNote records your microphone and, if you choose, the sound your computer plays, which includes the other participants of a call. Laws on recording conversations differ between countries and regions. Tell participants when you record, and get their consent where the law requires it.

## Deleting your data

Delete a meeting in the app to remove it from the database. The recording folder for that meeting stays on disk, so delete it as well if you want the audio gone. Uninstalling TriNote does not delete your data. To remove everything, delete the folders listed above.

## Changes to this policy

Changes to this policy are made in the [TriNote repository](https://github.com/TrinhHuuTho/trinote) and mentioned in [CHANGELOG.md](CHANGELOG.md).

## Contact

Questions about privacy: open an issue at <https://github.com/TrinhHuuTho/trinote/issues>.
