import SwiftUI

struct MainTabView: View {
    var body: some View {
        TabView {
            NavigationStack { KanbanView() }
                .tabItem { Label("Kanban", systemImage: "square.grid.3x3") }

            NavigationStack { ProjectsListView() }
                .tabItem { Label("Projects", systemImage: "folder") }

            NavigationStack { WalletView() }
                .tabItem { Label("Wallet", systemImage: "wallet.pass") }

            NavigationStack { NodesView() }
                .tabItem { Label("Nodes", systemImage: "cpu") }
        }
    }
}
