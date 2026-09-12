import SwiftUI
import UniformTypeIdentifiers

private enum TranscribeSource: String, CaseIterable, Identifiable {
    case onDevice = "On-device (Apple)"
    case network = "Hive network (whisper.cpp)"
    var id: String { rawValue }
}

/// UI for both transcription paths: Apple's on-device model (`TranscribeEngine.swift`, local-only,
/// free, private -- see its header comment for the ADR-018 guardrail on why it stays that way) and
/// the Hive network's whisper.cpp backend (`HiveStore.transcribeWhisper`, a direct call to
/// whatever `HIVE_WHISPER_URL` points at -- see `crates/ohhive-ffi/src/media.rs`'s header for why
/// this isn't yet full Hive-distributed job scheduling). One picker, one flow, whichever the
/// person wants -- the point is Hive being where you go to transcribe something, full stop.
struct TranscribeView: View {
    @EnvironmentObject private var store: HiveStore
    @StateObject private var engine = TranscribeEngine()
    @State private var source: TranscribeSource = .onDevice
    @State private var showingPicker = false
    @State private var networkTranscript = ""
    @State private var networkBusy = false
    @State private var networkError: String?

    private var whisperConfigured: Bool { !(store.snapshot?.whisperUrl?.isEmpty ?? true) }
    private var busy: Bool { source == .onDevice ? engine.isTranscribing : networkBusy }
    private var transcript: String { source == .onDevice ? engine.transcript : networkTranscript }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Picker("Source", selection: $source) {
                ForEach(TranscribeSource.allCases) { s in
                    Text(s.rawValue).tag(s)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()

            Text(source == .onDevice
                 ? "Runs on this Mac using Apple's built-in speech model -- free, private, nothing leaves this machine."
                 : "Runs against the whisper.cpp server configured in Settings > Media backends -- the same path other Hive nodes can offer the network.")
                .font(.caption)
                .foregroundStyle(.secondary)

            if source == .onDevice, let note = engine.statusNote {
                noteBox(note)
            }
            if source == .network, !whisperConfigured {
                noteBox("No whisper.cpp server configured yet. Set one in Settings > Media backends.")
            }
            if source == .network, let err = networkError {
                noteBox(err)
            }

            Button("Choose Audio File\u{2026}") { showingPicker = true }
                .disabled(busy || (source == .network && !whisperConfigured))

            if busy {
                HStack {
                    ProgressView().controlSize(.small)
                    Text("Transcribing\u{2026}").font(.caption).foregroundStyle(.secondary)
                }
            }

            ScrollView {
                Text(transcript.isEmpty ? "Transcript will appear here." : transcript)
                    .font(.callout)
                    .foregroundStyle(transcript.isEmpty ? .secondary : .primary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .textSelection(.enabled)
                    .padding(10)
            }
            .background(Color.secondary.opacity(0.05))
            .clipShape(RoundedRectangle(cornerRadius: 8))

            if !transcript.isEmpty {
                Button("Copy Transcript") {
                    #if canImport(AppKit)
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(transcript, forType: .string)
                    #endif
                }
            }
        }
        .padding()
        .navigationTitle("Transcribe")
        .task { await engine.checkAvailability() }
        .fileImporter(
            isPresented: $showingPicker,
            allowedContentTypes: [.audio, .mpeg4Audio, .wav],
            allowsMultipleSelection: false
        ) { result in
            guard case let .success(urls) = result, let url = urls.first else { return }
            let gotAccess = url.startAccessingSecurityScopedResource()
            switch source {
            case .onDevice:
                Task {
                    defer { if gotAccess { url.stopAccessingSecurityScopedResource() } }
                    await engine.transcribe(fileURL: url)
                }
            case .network:
                networkError = nil
                networkBusy = true
                Task {
                    defer {
                        if gotAccess { url.stopAccessingSecurityScopedResource() }
                        networkBusy = false
                    }
                    do {
                        let result = try await store.transcribeWhisper(audioPath: url.path)
                        networkTranscript = result.text
                    } catch {
                        networkError = "Transcription failed: \(error.localizedDescription)"
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func noteBox(_ text: String) -> some View {
        Text(text)
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.secondary.opacity(0.08))
            .clipShape(RoundedRectangle(cornerRadius: 6))
    }
}
