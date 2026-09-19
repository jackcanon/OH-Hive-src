import AppKit
import SwiftUI

/// Owns keyboard submission at the text editor, before SwiftUI shortcut routing.
final class BotsMessageTextView: NSTextView {
    var submit: (() -> Void)?
    var canSubmit = false

    override func keyDown(with event: NSEvent) {
        let newline = event.keyCode == 36 || event.keyCode == 76
        let editingModifiers: NSEvent.ModifierFlags = [.shift, .option, .control]
        if newline && event.modifierFlags.intersection(editingModifiers).isEmpty && !hasMarkedText() {
            if !event.isARepeat && canSubmit { submit?() }
            return
        }
        super.keyDown(with: event)
    }
}

struct BotsComposer: NSViewRepresentable {
    @Binding var text: String
    var label: String
    var canSend: Bool
    var send: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        let editor = BotsMessageTextView(frame: NSRect(x: 0, y: 0, width: 100, height: 60))
        editor.minSize = NSSize(width: 0, height: 60)
        editor.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        editor.isRichText = false
        editor.isAutomaticQuoteSubstitutionEnabled = false
        editor.isAutomaticDashSubstitutionEnabled = false
        editor.isAutomaticTextReplacementEnabled = false
        editor.allowsUndo = true
        editor.font = .preferredFont(forTextStyle: .body)
        editor.textColor = .labelColor
        editor.backgroundColor = .textBackgroundColor
        editor.textContainerInset = NSSize(width: 6, height: 6)
        editor.isVerticallyResizable = true
        editor.isHorizontallyResizable = false
        editor.autoresizingMask = [.width]
        editor.textContainer?.widthTracksTextView = true
        editor.textContainer?.containerSize = NSSize(width: 0, height: CGFloat.greatestFiniteMagnitude)
        editor.delegate = context.coordinator
        scroll.documentView = editor
        updateNSView(scroll, context: context)
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        context.coordinator.parent = self
        guard let editor = scroll.documentView as? BotsMessageTextView else { return }
        if editor.string != text && !editor.hasMarkedText() {
            editor.string = text
        }
        editor.canSubmit = canSend
        editor.submit = { [weak coordinator = context.coordinator] in
            guard let coordinator else { return }
            coordinator.parent.send()
        }
        editor.setAccessibilityLabel(label)
        editor.toolTip = "Enter to send; Shift+Enter for a new line."
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: BotsComposer
        init(_ parent: BotsComposer) { self.parent = parent }
        func textDidChange(_ notification: Notification) {
            guard let editor = notification.object as? NSTextView else { return }
            parent.text = editor.string
        }
    }
}
