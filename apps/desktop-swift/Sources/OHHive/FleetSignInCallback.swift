import Foundation

/// Invalid/duplicate/local stray requests never finish an enrollment attempt.
enum FleetSignInCallback {
    static func approval(_ request: String, state: String) -> String? {
        guard request.utf8.count <= 16384,
              let line = request.components(separatedBy:"\r\n").first else { return nil }
        let parts = line.split(separator:" ")
        guard parts.count == 3, parts[0] == "GET", parts[2] == "HTTP/1.1", parts[1].hasPrefix("/"),
              let url = URLComponents(string:"http://127.0.0.1" + parts[1]),
              url.host == "127.0.0.1", url.path == "/fleet-enrollment", url.fragment == nil else { return nil }
        let items = url.queryItems ?? []
        guard items.count == 2, items.filter({ $0.name == "state" }).count == 1,
              items.first(where: { $0.name == "state" })?.value == state,
              let code = items.first(where: { $0.name == "approval" })?.value,
              let data = Data(base64Encoded:code), data.count <= 10000,
              let text = String(data:data, encoding:.utf8), !text.isEmpty else { return nil }
        return text
    }
}
