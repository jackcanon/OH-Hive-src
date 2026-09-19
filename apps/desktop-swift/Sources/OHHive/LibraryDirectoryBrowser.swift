import SwiftUI
import AppKit

struct LibraryDirectoryBrowser: View {
    @Environment(\.dismiss) private var dismiss
    @State private var scans: [LibraryDirectoryScan] = []
    @State private var preview: LibraryDirectoryScan?
    @State private var query = ""
    @State private var kind = "All types"
    @State private var message: String?
    @State private var scanning = false
    @State private var scanTask: Task<Void, Never>?

    private var items: [LibraryDirectoryItem] {
        var byPath: [String: LibraryDirectoryItem] = [:]
        for scan in preview.map({ [$0] }) ?? scans { for item in scan.items { byPath[item.path] = item } }
        return byPath.values.filter {
            (kind == "All types" || $0.kind.rawValue == kind) &&
            (query.isEmpty || $0.path.localizedCaseInsensitiveContains(query))
        }.sorted { $0.path < $1.path }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Directories and assets").font(.title2)
                Spacer()
                Button("Done") { scanTask?.cancel(); dismiss() }
            }
            Text("Find repositories, Markdown notes and digital assets in a folder you choose. Saved entries refer to files on this Mac; source files stay where they are.")
                .foregroundStyle(.secondary)
            HStack {
                Button("Scan a folder…", action: chooseFolder).disabled(scanning)
                if scanning {
                    ProgressView().controlSize(.small)
                    Button("Cancel scan") { scanTask?.cancel() }
                }
                TextField("Search names and paths…", text: $query).textFieldStyle(.roundedBorder)
                Picker("Type", selection: $kind) {
                    Text("All types").tag("All types")
                    ForEach(LibraryDirectoryItem.Kind.allCases, id: \.self) { Text($0.rawValue).tag($0.rawValue) }
                }.frame(width: 190)
            }
            if let preview {
                Text("Review: \(preview.root)").font(.caption).textSelection(.enabled)
                if preview.incomplete {
                    Text("Partial scan: a scan limit or unreadable paths prevented complete coverage (\(preview.unreadable) read errors). Try a smaller folder.")
                        .foregroundStyle(.orange)
                }
                HStack {
                    Button("Save locations to Library") { keepPreview(preview) }.disabled(scanning)
                    Button("Discard results") { self.preview = nil }.disabled(scanning)
                    Text("\(preview.items.count) items found").foregroundStyle(.secondary)
                }
            } else if !scans.isEmpty {
                DisclosureGroup("Scanned folders (\(scans.count))") {
                    ForEach(scans, id: \.root) { scan in
                        HStack {
                            Text(scan.root).lineLimit(1).truncationMode(.middle)
                            Text(scan.scanned, style: .date).foregroundStyle(.secondary)
                            if scan.incomplete { Text("Partial").foregroundStyle(.orange) }
                            Button("Rescan") { startScan(URL(fileURLWithPath: scan.root)) }.disabled(scanning)
                            Button("Remove from catalog") { remove(scan.root) }.disabled(scanning)
                        }.font(.caption)
                    }
                }
            }
            if let message { Text(message).font(.caption).foregroundStyle(.secondary) }
            Text("\(items.count) matches · Asset contents are not indexed. Hidden folders, dependencies and build output are skipped.")
                .font(.caption).foregroundStyle(.secondary)
            List(items) { item in
                HStack {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(item.name).fontWeight(.medium)
                        Text(item.path).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                    }
                    Spacer()
                    Text(item.kind.rawValue).font(.caption)
                    if let bytes = item.bytes { Text(ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)).font(.caption) }
                    Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: item.path)]) }

                }
            }

        }
        .padding(20).frame(minWidth: 760, idealWidth: 900, minHeight: 500, idealHeight: 620)
        .task { do { scans = try LibraryDirectoryCatalog.load() } catch { message = "Could not read the saved catalog: \(error.localizedDescription)" } }
        .onDisappear { scanTask?.cancel() }
    }

    private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true; panel.canChooseFiles = false; panel.allowsMultipleSelection = false
        panel.message = "Choose the directory to discover. No files are imported during scanning."
        if panel.runModal() == .OK, let root = panel.url { startScan(root) }
    }
    private func startScan(_ root: URL) {
        scanning = true; message = nil; preview = nil
        scanTask = Task {
            let worker = Task.detached(priority: .utility) { try LibraryDirectoryScanner.scan(root: root) }
            do {
                let result = try await withTaskCancellationHandler(operation: { try await worker.value }, onCancel: { worker.cancel() })
                try Task.checkCancellation()
                preview = result
            } catch is CancellationError { message = "Scan canceled. Saved entries are unchanged." }
            catch { message = "Could not scan this folder: \(error.localizedDescription)" }
            scanning = false
        }
    }
    private func keepPreview(_ scan: LibraryDirectoryScan) {
        // Partial rescans retain previously discovered entries that were not reached this time.
        var saved = scan
        if scan.incomplete, let old = scans.first(where: { $0.root == scan.root }) {
            var entries = Dictionary(uniqueKeysWithValues: old.items.map { ($0.path, $0) })
            for item in scan.items { entries[item.path] = item }
            saved = .init(root: scan.root, scanned: scan.scanned, items: entries.values.sorted { $0.path < $1.path }, incomplete: true, unreadable: scan.unreadable)
        }
        let updated = scans.filter { $0.root != scan.root } + [saved]
        do { try LibraryDirectoryCatalog.save(updated); scans = updated; preview = nil; message = "Catalog saved. No source files were moved or changed." }
        catch { message = "Could not save the catalog: \(error.localizedDescription)" }
    }
    private func remove(_ root: String) {
        let updated = scans.filter { $0.root != root }
        do { try LibraryDirectoryCatalog.save(updated); scans = updated; message = "Removed from the catalog. Source files remain unchanged." }
        catch { message = "Could not save the catalog: \(error.localizedDescription)" }
    }
}
