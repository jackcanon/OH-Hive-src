import SwiftUI
import OHHiveFFI

/// How a node's working state is described to the member.
///
/// There are two different facts here and the app used to show only one. `workerEnabled` is what
/// the member asked for -- persisted, survives relaunches, changes only when they say so.
/// `workerState` is what the supervisor is actually doing about it right now. Before 2026-09-17
/// the UI had `running` alone, so "you switched it off" and "the loop died an hour ago" rendered
/// identically, and a node could sit dead for twelve hours looking exactly like a node at rest.
///
/// The button follows intent. The status line follows reality. That is the whole design.
extension HiveSnapshot {
    /// One line, in the member's terms, for the menu bar and the node card.
    var workerHeadline: String {
        switch workerState {
        case "working":
            return busy ? "Working — busy" : "Working"
        case "retrying":
            return workerDetail.map { "Retrying — \($0)" } ?? "Retrying"
        case "blocked":
            return workerDetail.map { "Needs attention — \($0)" } ?? "Needs attention"
        default:
            // Resting. Whether that is what the member wanted is a different question, and one
            // worth answering: intent on with nothing running means the supervisor is between
            // states, not that they forgot to press the button.
            return workerEnabled ? "Starting…" : "Not working"
        }
    }

    var workerSymbol: String {
        switch workerState {
        case "working": return busy ? "bolt.fill" : "checkmark.circle.fill"
        case "retrying": return "arrow.clockwise.circle.fill"
        case "blocked": return "exclamationmark.triangle.fill"
        default: return workerEnabled ? "clock.fill" : "pause.circle.fill"
        }
    }

    var workerTint: Color {
        switch workerState {
        case "working": return busy ? .orange : .green
        case "retrying": return .yellow
        case "blocked": return .red
        default: return .secondary
        }
    }

    /// What the toggle should say.
    ///
    /// Deliberately driven by intent, not by `running`: a node partway through a backoff has not
    /// stopped wanting to work, and offering "Start working" there would be asking the member to
    /// fix something the supervisor is already handling -- and pressing it would look like it did
    /// nothing, because intent was already set.
    var workerToggleTitle: String {
        workerEnabled ? "Stop working" : "Start working"
    }
}

/// The status line, with its icon. Shared so the menu bar and the node card cannot drift apart.
struct WorkerStatusLabel: View {
    let snapshot: HiveSnapshot

    var body: some View {
        Label {
            Text(snapshot.workerHeadline)
        } icon: {
            Image(systemName: snapshot.workerSymbol).foregroundStyle(snapshot.workerTint)
        }
        .font(.callout)
    }
}
