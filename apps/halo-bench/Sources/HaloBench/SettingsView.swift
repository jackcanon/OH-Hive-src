import SwiftUI
import AppKit

struct SettingsView: View {
    var body: some View {
        TabView {
            GeneralSettings().tabItem { Label("General", systemImage: "gearshape") }
            AboutView().tabItem { Label("About", systemImage: "info.circle") }
        }
        .frame(width: 520, height: 460)
    }
}

struct GeneralSettings: View {
    @Environment(BenchStore.self) private var store
    var body: some View {
        @Bindable var store = store
        Form {
            Section("Paths on this Mac") {
                TextField("llama-bench", text: $store.config.llamaBenchPath)
                TextField("Models folder", text: $store.config.modelsDir)
                TextField("llama.cpp tag (recorded in each report)", text: $store.config.llamaTag)
            }
            Section("Reports") {
                HStack {
                    TextField("Reports folder (source of truth for History)", text: $store.config.reportsDir)
                    Button("Choose…") {
                        let p = NSOpenPanel(); p.canChooseDirectories = true; p.canChooseFiles = false
                        if p.runModal() == .OK, let u = p.url { store.config.reportsDir = u.path; store.reload() }
                    }
                }
                Text("Every run files `YYYY-MM-DD_HH-mm_<title>.md` plus the raw `.log` here. Default is the repo's `docs/halo-reports/` so results are versioned with the ADRs.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section("Host") {
                Picker("This Mac in the fleet", selection: $store.config.localHostID) {
                    Text("Not set").tag(UUID?.none)
                    ForEach(store.config.fleet) { Text($0.name).tag(UUID?.some($0.id)) }
                }
                Text("Used for the placement line in reports and to keep this machine out of the worker list.").font(.caption).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .padding()
    }
}

/// Preferences -> About, and the app-menu "About HaloBench". Happy Jack Media house rule: a quiet
/// credit to the studio and a link to the film, on every app built for this team.
struct AboutView: View {
    var body: some View {
        VStack(spacing: 12) {
            LogoView(size: 72)
            Text("HaloBench").font(.title.bold())
            Text("v\(AppInfo.version) · Project Halo test bench for OH Hive").font(.caption).foregroundStyle(.secondary)
            Divider().padding(.vertical, 4)
            VStack(alignment: .leading, spacing: 10) {
                Text("Happy Jack Media is an independent documentary studio telling stories worth remembering — about veterans, communities, and the people whose stories tend to get left out of the larger narrative.")
                Text("Tools like this one are part of how that work gets funded. Time spent building software in-house is time and budget that doesn't have to come from someone else, so the films stay ours to tell, on our own terms.")
                Text("Our first feature, *This Is Not A Draft*, follows Post-9/11 veterans navigating life after service — streaming free.")
            }
            .font(.callout)
            .multilineTextAlignment(.leading)
            .fixedSize(horizontal: false, vertical: true)
            Link("Watch This Is Not A Draft", destination: URL(string: "https://www.thisisnotadraft.com")!)
                .font(.callout.bold())
            Link("happyjack.media", destination: URL(string: "https://www.happyjack.media")!)
                .font(.caption).foregroundStyle(.secondary)
        }
        .padding(24)
        .frame(maxWidth: 440)
    }
}
