import SwiftUI
import OHHiveFFI

struct SparkConnectorSettings: View {
    @EnvironmentObject private var store: HiveStore
    @ObservedObject var importer: SparkMeetingImporter
    @State private var vaults: [VaultInfo] = []
    @State private var vaultID = ""
    @State private var days = 30
    @State private var transcripts = false
    @State private var configuring = false
    @State private var error: String?

    var body: some View {
        GroupBox("Spark") {
            VStack(alignment: .leading, spacing: 10) {
                Text("Automatically save meeting summaries and notes to your Library. Keep Spark and Loki’s Den open on this Mac. No AI model is needed for importing.")
                    .font(.caption).foregroundStyle(.secondary)
                if importer.configuration.vaultID.isEmpty || configuring {
                    Text("In Spark, open Settings → AI Agents → Setup CLI. Allow Read access and meeting notes for the accounts you want to import.")
                        .font(.caption)
                    Link("Open Spark setup guide", destination: URL(string: "https://sparkmailapp.com/help/spark-cli/getting-started-with-spark-cli")!)
                    Button("Check Spark connection") { Task { await importer.checkConnection() } }
                    Picker("Save to collection", selection: $vaultID) {
                        Text("Choose a collection").tag("")
                        ForEach(vaults, id: \.id) { Text($0.name).tag($0.id) }
                    }
                    Button("Create Meeting Notes collection") {
                        do {
                            let vault = try store.vaultCreate(name: "Spark Meeting Notes")
                            vaults = store.vaultOpen()?.vaults ?? []
                            vaultID = vault.id
                        } catch { self.error = error.localizedDescription }
                    }
                    Picker("Import history", selection: $days) {
                        Text("Past week").tag(7)
                        Text("Past month").tag(30)
                        Text("Past 3 months").tag(90)
                        Text("Past year").tag(365)
                    }
                    Toggle("Include full transcripts", isOn: $transcripts)
                    Text("New meetings are checked every five minutes. Existing notes are refreshed once daily for edits. Imported copies stay in this Mac’s Library when removed from Spark. The collection’s sharing settings apply.")
                        .font(.caption).foregroundStyle(.secondary)
                    Button("Start automatic import") {
                        importer.connect(vaultID: vaultID, days: days, transcripts: transcripts)
                        configuring = false
                    }
                    .disabled(vaultID.isEmpty || importer.busy)
                } else {
                    Label(importer.configuration.enabled ? "Automatic import on · this Mac" : "Import paused", systemImage: "calendar.badge.clock")
                    Text("Collection: \(vaults.first(where: { $0.id == importer.configuration.vaultID })?.name ?? "Saved collection") · Since \(importer.configuration.since)")
                        .font(.caption)
                    HStack {
                        if importer.configuration.enabled {
                            Button("Sync now") { Task { await importer.sync() } }.disabled(importer.busy)
                            Button("Refresh existing notes") { Task { await importer.sync(refreshExisting: true) } }.disabled(importer.busy)
                            Button("Pause") { importer.pause() }
                        } else {
                            Button("Resume") { importer.resume() }.disabled(importer.busy)
                        }
                        Button("Change settings") { configuring = true }.disabled(importer.busy)
                    }
                }
                if importer.busy { ProgressView().controlSize(.small) }
                Text(importer.status).font(.caption).textSelection(.enabled)
                if let date = importer.configuration.lastSync {
                    Text("Last successful sync: \(date.formatted(date: .abbreviated, time: .shortened))").font(.caption).foregroundStyle(.secondary)
                }
                Divider()
                SparkEmailSettings(connector: store.sparkEmail)
                if let error { Text(error).font(.caption).foregroundStyle(.red) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .onAppear {
            vaults = store.vaultOpen()?.vaults ?? []
            vaultID = importer.configuration.vaultID
            transcripts = importer.configuration.transcripts
        }
    }
}
