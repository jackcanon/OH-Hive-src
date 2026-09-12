import SwiftUI
import UniformTypeIdentifiers

/// First-slice UI for on-device transcription (see `TranscribeEngine.swift` for the guardrail:
/// this is local-only, never Hive-distributed work). Deliberately plain, same spirit as
/// `ChatView` -- pick a file, get text back, copy it.
struct TranscribeView: View {
    @StateObject private var engine = TranscribeEngine()
    @State private var showingPicker = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Transcribe an audio file on this Mac, on-device, using Apple's built-in speech model -- free, private, nothing leaves this machine. This is separate from the Hive network's own speech-to-text (whisper.cpp), which runs jobs for other members and pays $honey.")
                .font(.caption)
                .foregroundStyle(.secondary)

            if let note = engine.statusNote {
                Text(note)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(8)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color.secondary.opacity(0.08))
                    .clipShape(RoundedRectangle(cornerRadius: 6))
            }

            Button("Choose Audio File\u{2026}") { showingPicker = true }
                .disabled(engine.isTranscribing)

            if engine.isTranscribing {
                HStack {
                    ProgressView().controlSize(.small)
                    Text("Transcribing\u{2026}").font(.caption).foregroundStyle(.secondary)
                }
            }

            ScrollView {
                Text(engine.transcript.isEmpty ? "Transcript will appear here." : engine.transcript)
                    .font(.callout)
                    .foregroundStyle(engine.transcript.isEmpty ? .secondary : .primary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .textSelection(.enabled)
                    .padding(10)
            }
            .background(Color.secondary.opacity(0.05))
            .clipShape(RoundedRectangle(cornerRadius: 8))

            if !engine.transcript.isEmpty {
                Button("Copy Transcript") {
                    #if canImport(AppKit)
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(engine.transcript, forType: .string)
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
            Task {
                let gotAccess = url.startAccessingSecurityScopedResource()
                defer { if gotAccess { url.stopAccessingSecurityScopedResource() } }
                await engine.transcribe(fileURL: url)
            }
        }
    }
}
