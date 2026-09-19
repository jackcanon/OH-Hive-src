import XCTest
@testable import Hive

final class LibraryDirectoryTests: XCTestCase {
    func testDiscoveryIncludesRootAndWorktreeRepositoriesWithoutFollowingLinks() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        for folder in [".git", "child", "node_modules", ".hidden", "assets"] {
            try FileManager.default.createDirectory(at: root.appendingPathComponent(folder), withIntermediateDirectories: true)
        }
        for file in ["README.MD", "child/.git", "child/notes.markdown", "node_modules/skip.md", ".hidden/skip.md", "assets/photo.PNG", "assets/film.mov", "assets/data.pdf", "source.swift"] {
            try Data("fixture".utf8).write(to: root.appendingPathComponent(file))
        }
        try FileManager.default.createSymbolicLink(at: root.appendingPathComponent("linked"), withDestinationURL: root.appendingPathComponent("assets"))
        try FileManager.default.createSymbolicLink(at: root.appendingPathComponent("linked.md"), withDestinationURL: root.appendingPathComponent("README.MD"))
        let scan = try LibraryDirectoryScanner.scan(root: root)
        XCTAssertFalse(scan.incomplete)
        XCTAssertEqual(scan.items.filter { $0.kind == .repository }.count, 2)
        XCTAssertEqual(scan.items.filter { $0.kind == .markdown }.count, 2)
        XCTAssertEqual(scan.items.count, 7)
        XCTAssertFalse(scan.items.contains { $0.path.contains("linked") || $0.name == "skip.md" })
    }
    func testBoundedScanReportsPartialAndRejectsRootSymlink() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let link = root.appendingPathExtension("link")
        defer { try? FileManager.default.removeItem(at: root); try? FileManager.default.removeItem(at: link) }
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        for n in 0..<5 { try Data().write(to: root.appendingPathComponent("\(n).md")) }
        XCTAssertTrue(try LibraryDirectoryScanner.scan(root: root, maximumEntries: 2).incomplete)
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: root)
        XCTAssertThrowsError(try LibraryDirectoryScanner.scan(root: link))
    }
    func testCatalogRoundTripRetainsMetadataAndProtectsPermissions() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("catalog.json")
        let item = LibraryDirectoryItem(path: "/example/design.psd", kind: .design, bytes: 123, modified: Date(timeIntervalSince1970: 100))
        let scan = LibraryDirectoryScan(root: "/example", scanned: Date(), items: [item], incomplete: false, unreadable: 0)
        try LibraryDirectoryCatalog.save([scan], to: file)
        XCTAssertEqual(try LibraryDirectoryCatalog.load(from: file).first?.items, [item])
        let mode = try FileManager.default.attributesOfItem(atPath: file.path)[.posixPermissions] as? NSNumber
        XCTAssertEqual(mode?.intValue, 0o600)
    }
}
