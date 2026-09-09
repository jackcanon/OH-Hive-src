import SwiftUI
import OHHiveFFI

/// Preferences -> About. Happy Jack Media house rule: credit the maker and link the blog, on
/// every app built for this team -- part of the definition of done, not an afterthought.
struct AboutView: View {
    @EnvironmentObject private var store: HiveStore

    var body: some View {
        let info = store.about
        VStack(spacing: 12) {
            Image(systemName: "hexagon.fill")
                .font(.system(size: 48))
                .foregroundStyle(.yellow)
            Text("OH Hive").font(.title.bold())
            Text("v\(info.appVersion) \u{00b7} core \(info.coreVersion)")
                .font(.caption).foregroundStyle(.secondary)
            Divider().padding(.vertical, 4)
            Text("Made by")
                .font(.caption).foregroundStyle(.secondary)
            Link(info.madeBy, destination: URL(string: info.madeByUrl)!)
                .font(.callout.bold())
            Link(info.blogName, destination: URL(string: info.blogUrl)!)
                .font(.caption)
        }
        .padding(32)
        .frame(width: 320)
    }
}
