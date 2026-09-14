import SwiftUI

/// Every filed report, newest first, with the human-readable writeup on the right.
/// Reads straight from `docs/halo-reports/` -- edit a file there and hit Reload.
struct HistoryView: View {
    @Environment(BenchStore.self) private var store
    @State private var selection: TestReport?
    @State private var filter: Outcome?
    @State private var query = ""

    private var filtered: [TestReport] {
        store.reports.filter { r in
            (filter == nil || r.outcome == filter) &&
            (query.isEmpty || r.title.localizedCaseInsensitiveContains(query) || r.model.localizedCaseInsensitiveContains(query) || r.placement.localizedCaseInsensitiveContains(query))
        }
    }

    var body: some View {
        HSplitView {
            VStack(spacing: 0) {
                HStack {
                    Picker("", selection: $filter) {
                        Text("All").tag(Outcome?.none)
                        ForEach(Outcome.allCases, id: \.self) { Text($0.label).tag(Outcome?.some($0)) }
                    }.pickerStyle(.segmented).labelsHidden()
                }.padding(10)
                List(filtered, selection: $selection) { r in
                    ReportRow(report: r).tag(r)
                }
                .searchable(text: $query, placement: .sidebar, prompt: "Title, model, placement")
            }
            .frame(minWidth: 300, idealWidth: 380)

            Group {
                if let r = selection ?? store.reports.first {
                    ReportDetail(report: r)
                } else {
                    ContentUnavailableView("No reports", systemImage: "doc.text", description: Text("Reports filed by the Run tab appear here."))
                }
            }
            .frame(minWidth: 380)
        }
        .navigationTitle("History")
        .toolbar {
            Button { store.reload() } label: { Label("Reload", systemImage: "arrow.clockwise") }
            Button { NSWorkspace.shared.open(store.config.reportsURL) } label: { Label("Open folder", systemImage: "folder") }
        }
    }
}

struct ReportDetail: View {
    var report: TestReport
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                HStack(alignment: .top) {
                    Image(systemName: report.outcome.symbol).font(.title).foregroundStyle(report.outcome.color)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(report.title).font(.title2.bold())
                        Text(report.date.formatted(date: .long, time: .shortened)).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Open in editor") { NSWorkspace.shared.open(report.fileURL) }
                }
                HStack(spacing: 12) {
                    Stat(label: "Decode tg128", value: report.tg128.map { ReportStore.num($0) + " tok/s" } ?? "—")
                    Stat(label: "Prefill pp512", value: report.pp512.map { ReportStore.num($0) + " tok/s" } ?? "—")
                    Stat(label: "Model", value: report.model.isEmpty ? "—" : report.model)
                    Stat(label: "Placement", value: report.placement.isEmpty ? "—" : report.placement)
                    if !report.tensorSplit.isEmpty { Stat(label: "tensor-split", value: report.tensorSplit) }
                }
                if !report.summary.isEmpty {
                    GroupBox("Summary") { Text(report.summary).frame(maxWidth: .infinity, alignment: .leading).textSelection(.enabled) }
                }
                GroupBox("Full report") {
                    Text(rendered(report.body)).font(.system(.body, design: .default)).frame(maxWidth: .infinity, alignment: .leading).textSelection(.enabled)
                }
            }
            .padding(20)
        }
    }

    private func rendered(_ md: String) -> AttributedString {
        (try? AttributedString(markdown: md, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(md)
    }
}

struct Stat: View {
    var label: String; var value: String
    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label).font(.caption2).foregroundStyle(.secondary)
            Text(value).font(.system(.callout, design: .monospaced)).lineLimit(2)
        }
        .padding(8).background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 8))
    }
}
