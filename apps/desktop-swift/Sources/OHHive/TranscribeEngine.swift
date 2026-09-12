import Foundation
import Speech
import AVFoundation

/// M8 (media backends) first slice of the *local-only* transcription path: Apple's on-device
/// `SpeechAnalyzer`/`SpeechTranscriber` (Speech framework, shipping since macOS 26, the API
/// Jack asked to make sure gets used alongside whisper.cpp rather than treating whisper as the
/// only option). Same split as `ChatEngine.swift`'s Foundation Models chat: this transcribes a
/// file the node owner picked, on their own Mac, for free and privately -- it is NOT wired into
/// Hive-distributed job scheduling and must never be, per the ADR-018 guardrail (decision 11):
/// Apple's on-device frameworks may only serve `execution_mode='local'` runs on the owner's own
/// machine, never another member's paid, Hive-distributed card. The actual Hive-distributed
/// Speech modality -- a node earning $honey by transcribing *other* members' cards -- is
/// `crates/ohhive-core/src/backend/whisper.rs`, which works on every OS this project supports
/// (Windows/Linux/Intel Mac included) precisely because it isn't an Apple-only API.
///
/// Caveat, stated plainly rather than buried: this compiles clean against the real macOS 27 SDK's
/// Speech framework (`swift build` on Midgaard, not just reasoned about from WWDC 2025 material --
/// one real mismatch got caught and fixed that way: `AssetInventory.status(forModules:)`, not
/// `forLocale:`). What compiling does NOT confirm is runtime behavior against a real audio file --
/// nobody has fed this a .wav yet, so treat "does it actually transcribe correctly" as still open,
/// same as any first slice.
@MainActor
final class TranscribeEngine: ObservableObject {
    @Published var transcript: String = ""
    @Published var isTranscribing = false
    @Published var statusNote: String?

    private let locale = Locale(identifier: "en-US")

    /// Whether the on-device model for `locale` is installed yet. Checked lazily rather than at
    /// init so the app doesn't pay this cost (or prompt a download) until the owner actually
    /// tries to transcribe something.
    func checkAvailability() async {
        let transcriber = SpeechTranscriber(
            locale: locale,
            transcriptionOptions: [],
            reportingOptions: [],
            attributeOptions: []
        )
        let status = await AssetInventory.status(forModules: [transcriber])
        switch status {
        case .installed:
            statusNote = nil
        case .supported:
            statusNote = "On-device speech model not downloaded yet -- it will download the first time you transcribe."
        case .unsupported:
            statusNote = "This locale isn't supported for on-device transcription on this Mac."
        default:
            statusNote = "On-device transcription status unknown."
        }
    }

    /// Transcribe a local audio file end to end: ensure the model asset is present, run it
    /// through `SpeechAnalyzer`, collect the final text. Never touches the network beyond the
    /// one-time model asset download Apple's own framework performs.
    func transcribe(fileURL: URL) async {
        isTranscribing = true
        transcript = ""
        defer { isTranscribing = false }

        let transcriber = SpeechTranscriber(
            locale: locale,
            transcriptionOptions: [],
            reportingOptions: [],
            attributeOptions: []
        )

        do {
            if let request = try await AssetInventory.assetInstallationRequest(supporting: [transcriber]) {
                statusNote = "Downloading on-device speech model\u{2026}"
                try await request.downloadAndInstall()
                statusNote = nil
            }

            let analyzer = SpeechAnalyzer(modules: [transcriber])
            let audioFile = try AVAudioFile(forReading: fileURL)

            // Feeds the file through buffer-by-buffer via `AnalyzerInput`/`start(inputSequence:)` --
            // compiles against the real SDK, but hasn't been run against actual audio yet, so
            // whether 4096-frame chunks are the right granularity (vs. the file's own buffer size,
            // or a duration-based chunk) is still an open question for the first real test.
            let inputs = AsyncStream<AnalyzerInput> { continuation in
                Task {
                    do {
                        while let buffer = try audioFile.readBuffer() {
                            continuation.yield(AnalyzerInput(buffer: buffer))
                        }
                    } catch {
                        // Best-effort: end the stream, let `results` below report what it has.
                    }
                    continuation.finish()
                }
            }
            try await analyzer.start(inputSequence: inputs)

            for try await result in transcriber.results {
                transcript = result.text.description
            }
        } catch {
            statusNote = "Transcription failed: \(error.localizedDescription)"
        }
    }
}

private extension AVAudioFile {
    /// Reads the next chunk of the file as a PCM buffer, or `nil` at end of file. Small helper
    /// so `transcribe(fileURL:)` above stays readable.
    func readBuffer(frameCount: AVAudioFrameCount = 4096) throws -> AVAudioPCMBuffer? {
        guard framePosition < length else { return nil }
        guard let buffer = AVAudioPCMBuffer(pcmFormat: processingFormat, frameCapacity: frameCount) else {
            return nil
        }
        try read(into: buffer)
        return buffer.frameLength > 0 ? buffer : nil
    }
}
