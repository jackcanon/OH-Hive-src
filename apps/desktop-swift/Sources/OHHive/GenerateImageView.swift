import SwiftUI
import OHHiveFFI
#if canImport(AppKit)
import AppKit
#endif

/// UI for Hive's two image-generation paths (tasks #125-128): hosted (OpenAI, via the hub,
/// `HiveStore.generateImageHosted`) is the default -- no setup, paid for out of Honey. Local
/// (`HiveStore.generateImageComfyUI`, `crates/ohhive-ffi/src/media.rs`) is the free advanced
/// option for anyone who's pointed `HIVE_COMFYUI_URL` at their own running ComfyUI instance (see
/// that file's header for why this isn't yet full Hive-distributed job scheduling). Otherwise
/// plain on purpose: a prompt, an optional negative prompt, a Generate button, a picture.
struct GenerateImageView: View {
    @EnvironmentObject private var store: HiveStore
    private enum Source: String, CaseIterable, Identifiable { case hosted = "Hosted", local = "Local (ComfyUI)"; var id: String { rawValue } }
    @State private var source: Source = .hosted
    @State private var prompt = ""
    @State private var negativePrompt = ""
    @State private var busy = false
    @State private var error: String?
    @State private var image: NSImage?
    @State private var lastComputeSeconds: Double?

    private var comfyuiConfigured: Bool { !(store.snapshot?.comfyuiUrl?.isEmpty ?? true) }
    private var canGenerate: Bool { source == .hosted || comfyuiConfigured }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Picker("Source", selection: $source) {
                ForEach(Source.allCases) { Text($0.rawValue).tag($0) }
            }
            .pickerStyle(.segmented)
            .disabled(busy)

            Text(source == .hosted
                 ? "Generates an image through Hive's hosted path (OpenAI) -- no setup needed, charged to your Honey wallet."
                 : "Generates an image using the ComfyUI server configured in Settings > Media backends. This can take a while on modest hardware -- the request runs on whatever machine you pointed it at.")
                .font(.caption)
                .foregroundStyle(.secondary)

            if source == .local && !comfyuiConfigured {
                noteBox("No ComfyUI server configured yet. Set one in Settings > Media backends.")
            }
            if let error {
                noteBox(error)
            }

            Text("Prompt").font(.caption).foregroundStyle(.secondary)
            TextField("A watercolor fox in a snowy forest\u{2026}", text: $prompt, axis: .vertical)
                .textFieldStyle(.roundedBorder)
                .lineLimit(2...4)
                .disabled(busy)

            Text("Negative prompt (optional)").font(.caption).foregroundStyle(.secondary)
            TextField("blurry, low quality\u{2026}", text: $negativePrompt, axis: .vertical)
                .textFieldStyle(.roundedBorder)
                .lineLimit(1...3)
                .disabled(busy)

            HStack {
                Button("Generate") { Task { await generate() } }
                    .disabled(busy || prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !canGenerate)
                if busy {
                    ProgressView().controlSize(.small)
                    Text("Generating\u{2026} this can take a minute or more.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                if let lastComputeSeconds, !busy {
                    Text("Done in \(String(format: "%.1f", lastComputeSeconds))s")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            ScrollView {
                Group {
                    if let image {
                        Image(nsImage: image)
                            .resizable()
                            .aspectRatio(contentMode: .fit)
                    } else {
                        Text("Your generated image will appear here.")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, minHeight: 200, alignment: .center)
                    }
                }
                .frame(maxWidth: .infinity)
            }
            .background(Color.secondary.opacity(0.05))
            .clipShape(RoundedRectangle(cornerRadius: 8))
        }
        .padding()
        .navigationTitle("Generate")
    }

    private func generate() async {
        error = nil
        busy = true
        defer { busy = false }
        do {
            let negative = negativePrompt.trimmingCharacters(in: .whitespacesAndNewlines)
            let result = source == .hosted
                ? try await store.generateImageHosted(prompt: prompt, negativePrompt: negative.isEmpty ? nil : negative)
                : try await store.generateImageComfyUI(prompt: prompt, negativePrompt: negative.isEmpty ? nil : negative)
            lastComputeSeconds = result.computeSeconds
            image = NSImage(contentsOfFile: result.filePath)
            if image == nil {
                error = "Generated, but couldn't load the image file at \(result.filePath)."
            }
        } catch {
            self.error = "Image generation failed: \(error.localizedDescription)"
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
