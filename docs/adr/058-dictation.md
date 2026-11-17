# ADR-058: Dictation

## Status

Accepted, 2026-11-18. Adds a seventh app to ADR-042 and a new kind of outgoing data to ADR-031.

## Context

Speech to text is offered by OpenAI-compatible `audio/transcriptions` endpoints (OpenAI, Groq, self-hosted servers) and, from macOS 26, by SpeechAnalyzer on the Mac itself. Anthropic has no speech-to-text endpoint. Audio says more than the text a person has already typed, and capture needs a native stack on each of three platforms.

## Decision

1. **The seventh app.** `dictation` is last in `AppId::ALL`, off on a fresh install, with no offer. While it is off, its menu item, palette entry and status-bar indicator are absent and its chord `CmdOrCtrl+Shift+.` opens Settings at its switch, as for every other app.
2. **Two engines, one event stream.** `dictation_start` returns a session and streams `DictationEvent` over a Tauri `Channel` made for that session. The API engine posts each segment to `{base}/audio/transcriptions`; the on-device engine feeds SpeechAnalyzer through the Swift bridge. The frontend handles both the same way.
3. **Engine when unset.** `dictation.engine` is optional. Absent, the engine is on-device where this Mac supports it and API elsewhere, decided by one pure function in `writ-core` that `dictation_start`, the capabilities payload and Settings all use. Choosing an engine in Settings writes the key.
4. **Capture with cpal** in `crates/writ-audio`, which has no Tauri dependency: CoreAudio, WASAPI and ALSA behind one path, downmixed and resampled with rubato to 16 kHz mono Int16 for both engines. macOS needs `com.apple.security.device.audio-input` under the hardened runtime and `NSMicrophoneUsageDescription`. Linux needs `libasound2` at run time (a package dependency; the AppImage uses the host's copy) and reaches PipeWire or PulseAudio through ALSA's `default` device. The binary links `libasound.so.2` dynamically and the AppImage leaves it out, so the AppImage needs `libasound2` (`libasound2t64` on Ubuntu 24.04, `alsa-lib` elsewhere) to start, even with Dictation off; the download page and the dictation docs say so.
5. **Segments at pauses.** `writ_core::dictation::Segmenter` closes a segment after 800 ms of silence once it holds 3 s of speech, cuts at 30 s on the quietest frame, and drops a segment with under 250 ms of speech. Segments are transcribed one at a time, in order, each prompted with the end of the text before it.
6. **Audio stays in memory.** Samples and encoded segments are never written to disk or logged.
7. **Its own consent list.** Audio goes to a hosted endpoint only after the person allows that host in `dictation.consented_hosts`, Rust-owned like `ai.consented_hosts`. A host allowed for text is not thereby allowed for audio. The optional clean-up pass sends the transcript through the AI connection, under that connection's endpoint, consent and key checks.
8. **Key reuse.** When the dictation provider is the AI connection's provider, both resolve to the same address (scheme, host and port), and Chat or Rewrite is on, dictation uses the AI connection's keychain entry and Settings shows no second field. Otherwise it keeps its own entry and its Settings page shows the key field: one per preset provider (`dictation.openai`, `dictation.groq`) and, for `custom`, one per address, so a key typed for one server is never sent to another after the base URL changes.
9. **Insertion, stop and cancel.** Each segment is inserted at a tracked anchor in the pane that started the session, one undo step per segment. Escape anywhere in the window cancels: what is not inserted yet is discarded and inserted segments stay. It is left to the other layer while a dialog, the palette, Find, a menu or the dictating editor's completion list is open, while the focus is in a text field outside the editor, and during an IME composition. A tab change or switching the app off cancels the same way. Hiding, minimizing or closing the main window stops the session as a second press does: capture ends and what was spoken is still transcribed and inserted. Rust alone decides this, on those window actions; a window that is covered, on another Space or unfocused keeps listening. A session with no speech for 60 seconds stops the same way, on both engines, with the notice "Dictation stopped after a minute of silence." On every stop path Rust sends `stopping` once capture has ended, and the page shows the session as stopping until its last event.

## Consequences

- On-device dictation needs macOS 26. SpeechAnalyzer detects no language, so Automatic means the system language on the Mac; on the API engine the service detects it per segment.
- A denied microphone on macOS reads as silence rather than an error, so 1.5 s of digital silence ends the session with a link to the microphone setting.
- On-device speech models download through `AssetInventory` on first use, shown as a percentage in the status bar.
- Groq bills at least 10 s per request, so short segments cost more there than their length.
