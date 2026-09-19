import Foundation

struct LibraryDirectoryItem: Codable, Identifiable, Equatable, Sendable {
    enum Kind: String, Codable, CaseIterable, Sendable {
        case repository = "Repository", markdown = "Markdown", image = "Image", audio = "Audio"
        case video = "Video", document = "Document", design = "Design", archive = "Archive"
    }
    let path: String
    let kind: Kind
    let bytes: Int64?
    let modified: Date?
    var id: String { path }
    var name: String { URL(fileURLWithPath: path).lastPathComponent }
}

struct LibraryDirectoryScan: Codable, Sendable {
    let root: String
    let scanned: Date
    let items: [LibraryDirectoryItem]
    let incomplete: Bool
    let unreadable: Int
}

/// Metadata discovery only: never runs git, opens asset contents, or imports notes.
enum LibraryDirectoryScanner {
    static let excluded: Set<String> = ["node_modules", "vendor", "target", "build", "dist", "DerivedData", "Pods", "__pycache__"]
    static func kind(extension ext: String) -> LibraryDirectoryItem.Kind? {
        switch ext.lowercased() {
        case "md", "markdown": return .markdown
        case "png", "jpg", "jpeg", "gif", "webp", "heic", "tif", "tiff", "svg", "avif": return .image
        case "mp3", "wav", "aiff", "aif", "m4a", "flac", "ogg": return .audio
        case "mp4", "mov", "m4v", "mkv", "webm", "avi": return .video
        case "pdf", "txt", "rtf", "doc", "docx", "xls", "xlsx", "csv", "ppt", "pptx", "pages", "numbers", "key": return .document
        case "psd", "ai", "sketch", "fig", "blend", "obj", "stl", "usdz": return .design
        case "zip", "tar", "gz", "7z": return .archive
        default: return nil
        }
    }
    static func scan(root: URL, maximumEntries: Int = 50_000) throws -> LibraryDirectoryScan {
        let fm = FileManager.default
        let root = root.standardizedFileURL
        let keys: Set<URLResourceKey> = [.isDirectoryKey, .isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey, .contentModificationDateKey, .isPackageKey]
        let info = try root.resourceValues(forKeys: keys)
        guard info.isDirectory == true, info.isSymbolicLink != true else {
            throw CocoaError(.fileReadUnsupportedScheme)
        }
        var items: [LibraryDirectoryItem] = []
        var unreadable = 0
        var incomplete = false
        func repository(_ directory: URL) {
            let marker = directory.appendingPathComponent(".git")
            if let attributes = try? fm.attributesOfItem(atPath: marker.path),
               let type = attributes[.type] as? FileAttributeType,
               type == .typeDirectory || type == .typeRegular {
                items.append(.init(path: directory.path, kind: .repository, bytes: nil, modified: nil))
            }
        }
        repository(root)
        guard let walker = fm.enumerator(at: root, includingPropertiesForKeys: Array(keys),
                                         options: [.skipsHiddenFiles, .skipsPackageDescendants],
                                         errorHandler: { _, _ in unreadable += 1; return true }) else {
            throw CocoaError(.fileReadNoPermission)
        }
        var visited = 0
        for case let url as URL in walker {
            try Task.checkCancellation()
            visited += 1
            if visited > maximumEntries { incomplete = true; break }
            do {
                let values = try url.resourceValues(forKeys: keys)
                if values.isSymbolicLink == true { walker.skipDescendants(); continue }
                if values.isDirectory == true {
                    if excluded.contains(url.lastPathComponent) || values.isPackage == true {
                        walker.skipDescendants(); continue
                    }
                    repository(url)
                } else if values.isRegularFile == true, let kind = kind(extension: url.pathExtension) {
                    items.append(.init(path: url.path, kind: kind, bytes: values.fileSize.map(Int64.init), modified: values.contentModificationDate))
                }
            } catch { unreadable += 1 }
        }
        return .init(root: root.path, scanned: Date(), items: items.sorted { $0.path < $1.path },
                     incomplete: incomplete || unreadable > 0, unreadable: unreadable)
    }
}

/// Separate local metadata catalog; does not grant agent access to source files.
enum LibraryDirectoryCatalog {
    static var url: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/ohhive/library-directories.json")
    }
    static func load(from file: URL = url) throws -> [LibraryDirectoryScan] {
        guard FileManager.default.fileExists(atPath: file.path) else { return [] }
        return try JSONDecoder().decode([LibraryDirectoryScan].self, from: Data(contentsOf: file))
    }
    static func save(_ scans: [LibraryDirectoryScan], to file: URL = url) throws {
        try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true,
                                                attributes: [.posixPermissions: 0o700])
        try JSONEncoder().encode(scans).write(to: file, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
    }
}
