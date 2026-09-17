import Foundation

@main struct FleetSignInTests {
    static func main() {
        let state = "9D1B7AD5-87F1-43D4-BEA0-DA74C38E98BD"
        let assertion = "{\"signature\":\"test-only\"}"
        let code = Data(assertion.utf8).base64EncodedString()
        var url = URLComponents(string: "http://127.0.0.1/fleet-enrollment")!
        url.queryItems = [URLQueryItem(name: "state", value: state), URLQueryItem(name: "approval", value: code)]
        let target = url.percentEncodedPath + "?" + url.percentEncodedQuery!
        func request(_ target: String, method: String = "GET") -> String {
            "\(method) \(target) HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
        }
        precondition(FleetSignInCallback.approval(request(target), state: state) == assertion)
        for invalid in [request(target, method: "POST"), request(target + "&state=" + state),
                        request(target + "&approval=" + code), request("/favicon.ico"),
                        request(target.replacingOccurrences(of: state, with: "wrong")),
                        request("/fleet-enrollment?state=" + state + "&approval=invalid!"),
                        request("http://evil.example" + target), request(target + "#fragment"),
                        String(repeating: "x", count: 16385)] {
            precondition(FleetSignInCallback.approval(invalid, state: state) == nil)
        }
        print("Fleet sign-in callback: valid round trip and 9 rejection cases passed")
    }
}
