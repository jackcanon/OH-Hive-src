import XCTest
@testable import Hive

final class SparkMeetingTests: XCTestCase {
    func testPaginationWithTruncatedTitlesAndUnicode() throws {
        let result = try SparkMeetingFormat.page("""
        Meeting Notes
          ID Title Date Duration
          42  Weekly notes…  2026-09-18 09:30  10m
          99  Café planning  2026-09-17 10:00  2h
        Page 1 of 23 (1131 total meetings)
        """)
        XCTAssertEqual(result.ids, ["42", "99"])
        XCTAssertEqual(result.pages, 23)
    }
    func testEmptyAndUnexpectedOutput() throws {
        XCTAssertEqual(try SparkMeetingFormat.page("\nMeeting Notes (filter: after:2099/01/01)\n\nNo meetings found.\n").ids, [])
        for text in ["Access denied", "", "Meeting Notes\nNo meetings found.\nerror", "Page 1 of 900 (45000 total meetings)", "42 hello\nPage 1 of 1 (1 total meetings)"] {
            XCTAssertThrowsError(try SparkMeetingFormat.page(text))
        }
    }
    func testStableMarkdownPreservesSourceAndEdits() throws {
        let raw = "Meeting: Planning\nDate: 2026-09-18\nLink: https://example.test/meeting\n\nDecision: ship the prototype."
        let first = try SparkMeetingFormat.markdown(raw, id: "42")
        XCTAssertEqual(first, try SparkMeetingFormat.markdown(raw, id: "42"))
        XCTAssertTrue(first.contains(raw))
        XCTAssertTrue(first.hasPrefix("# Planning\n"))
        XCTAssertTrue(first.contains("Source: Spark meeting 42"))
        XCTAssertNotEqual(first, try SparkMeetingFormat.markdown(raw + "\nUpdated notes.", id: "42"))
        XCTAssertThrowsError(try SparkMeetingFormat.markdown("Please enable access", id: "42"))
        XCTAssertThrowsError(try SparkMeetingFormat.markdown(raw + String(repeating: "x", count: 950_000), id: "42"))
    }
    func testConfigurationRetainsSourceNamespaceAndDefaultsOff() throws {
        let c = SparkImportConfiguration()
        XCTAssertFalse(c.enabled)
        XCTAssertFalse(c.transcripts)
        let restored = try JSONDecoder().decode(SparkImportConfiguration.self, from: JSONEncoder().encode(c))
        XCTAssertEqual(restored.namespace, c.namespace)
    }
}
