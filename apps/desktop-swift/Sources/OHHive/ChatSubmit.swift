import SwiftUI

extension View {
    /// Scope Return to the focused composer; Shift-Return keeps native multiline editing.
    func chatSubmit(enabled: Bool, action: @escaping () -> Void) -> some View {
        onKeyPress(.return, phases: .down) { press in
            guard !press.modifiers.contains(.shift),
                  !press.modifiers.contains(.option),
                  !press.modifiers.contains(.control) else { return .ignored }
            if enabled { action() }
            return .handled
        }
        .help("Enter to send; Shift+Enter for a new line.")
    }
}
