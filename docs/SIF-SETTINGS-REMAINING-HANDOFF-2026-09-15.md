# Settings T-3, T-4 and T-6; T-5 dependency

Implemented:

- Providers combines the existing ChatGPT subscription connection and native API-key controls. Provider labels now share providerDisplayName with the chat model picker. Existing authentication/storage semantics are preserved; grouping does not imply ChatGPT agent replies are implemented.
- Models uses HiveStore.assess/ollamaInstall/ollamaPull and the existing snapshot model catalog. It shows runtime state, installed/default models, missing selected models, hardware-based recommendation/download size, explicit install/start/download buttons, progress, refresh and error feedback. No automatic install/download and no separate model-discovery implementation. This catalog should feed the upcoming per-agent picker.
- Advanced replaces Backend, with its raw controls in a collapsed disclosure. Model selection leaves General. Media endpoints remain in Media.
- Shared SettingsNote, providerDisplayName and formatStorageGB remove the repeated settings-related helpers across image generation, transcription, feedback, setup, server and provider selection. Small-value storage formatting now consistently uses two decimals.
- Image help and native missing-key error point to Providers. Transcription calls its existing non-Apple route Configured server, explicitly explains direct file transfer rather than Hive-project scheduling, and disables changing the destination during transcription.

During the build, concurrent core work changed backend::collect from a tuple to Completion. Updated only the two FFI media callers to destructure text/usage/truncated and reject truncated media results. Other-agent core edits were left intact.

T-5 is NOT implemented. See SIF-HIVE-TRANSCRIPTION-IMPLEMENTATION-2026-09-15.md for the concrete missing path and implementation sequence. Jack selected any eligible volunteer in the Hive; membership remains invitation-only. The current artifact uploader forwards a reusable node key to discovered regional storage; do not enable audio distribution with that unchanged. No invitation or membership behavior is changed, and no Hive audio upload, model download or provider call was made during implementation.

Verification and final build status are recorded in continuity. UI interaction, runtime install/download and live provider/transcription acceptance remain untested; compilation is not evidence of those live flows.

Sif your friendly Codex Agent
