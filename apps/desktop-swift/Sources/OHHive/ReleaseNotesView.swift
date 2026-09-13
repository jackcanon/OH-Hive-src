import SwiftUI
import OHHiveFFI

/// The one-time "what's new" sheet (#178, 2026-09-13 -- Jack: "when users login after an update
/// there should be release notes"). `ContentView` fetches `releaseNotesUnseen()` once per launch
/// (as soon as the store has a paired snapshot) and presents this as a `.sheet` when the list
/// isn't empty; dismissing here calls `releaseNotesMarkSeen()` so it won't come back on this
/// machine, or any other this member signs into, until a new note ships.
struct ReleaseNotesView: View {
    let notes: [ReleaseNote]
    let onDismiss: () async -> Void
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 4) {
                Text("What's new").font(.title2).bold()
                Text("Here's what's changed in the Hive since you last signed in.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .padding([.horizontal, .top], 20)
            .padding(.bottom, 12)

            Divider()

            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    ForEach(notes, id: \.seq) { note in
                        VStack(alignment: .leading, spacing: 4) {
                            HStack(alignment: .firstTextBaseline, spacing: 6) {
                                Text(note.title).font(.headline)
                                Text("v\(note.version)").font(.caption).foregroundStyle(.secondary)
                            }
                            Text(note.bodyMd).font(.callout).foregroundStyle(.secondary)
                        }
                    }
                }
                .padding(20)
            }

            Divider()

            HStack {
                Spacer()
                Button("Got it") { Task { await dismiss() } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(busy)
                if busy { ProgressView().controlSize(.small) }
            }
            .padding(16)
        }
        .frame(width: 460, height: 420)
    }

    private func dismiss() async {
        busy = true
        await onDismiss()
    }
}
