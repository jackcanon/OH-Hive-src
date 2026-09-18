import SwiftUI

struct PrivateFleetEnrollmentView: View {
    @EnvironmentObject private var store: HiveStore
    @StateObject private var signIn = PrivateFleetSignIn()
    @State private var addingComputer = false

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            if addingComputer {
                Button("Back") { addingComputer = false }
                PrivatePrimaryView(initialAction: "join")
            } else if store.snapshot?.privateFleetEnrolled == true || signIn.completed {
                PrivatePrimaryView()
            } else {
                Text("Make this Mac yours").font(.title2)
                Text("Sign in with Google or Apple to register this computer and start your private projects.")
                    .foregroundStyle(.secondary)
                if signIn.busy {
                    ProgressView("Finish signing in in your browser…")
                    Text("Approve this computer there. We’ll finish registration here automatically.").font(.caption).foregroundStyle(.secondary)
                    Button("Cancel") { signIn.cancel() }
                } else {
                    Button("Sign in to register this Mac") { signIn.start(store:store) }
                        .buttonStyle(.borderedProminent)
                    Button("I’m adding a computer to an existing fleet") { addingComputer = true }
                        .buttonStyle(.link)
                }
                Text("Community Hives remain invite-only. Registration does not join one.").font(.caption).foregroundStyle(.secondary)
            }
            if let message = signIn.message { Text(message).font(.callout).textSelection(.enabled) }
        }.frame(maxWidth:.infinity, alignment:.leading)
        .onDisappear { signIn.cancel() }
    }
}
