import SwiftUI
import OHHiveFFI

/// Preferences -> About. Happy Jack Media house rule: credit the maker and link the blog, on
/// every app built for this team -- part of the definition of done, not an afterthought.
struct AboutView: View {
    @EnvironmentObject private var store: HiveStore

    var body: some View {
        let info = store.about
        VStack(spacing: 12) {
            Image(systemName: "door.left.hand.closed")
                .font(.system(size: 48))
                .foregroundStyle(Color(red: 0.824, green: 0.404, blue: 0.263))
            Text("Loki's Den").font(.title.bold())
            Text("v\(info.appVersion) \u{00b7} core \(info.coreVersion)")
                .font(.caption).foregroundStyle(.secondary)
            // Which source this bundle came from, because twice in one evening the app was the
            // odd one out -- carrying a core older than the rest of the fleet and contradicting
            // it -- and the only way to find out was running `strings` on a dylib. A build from
            // an uncommitted tree says so, since that is the build that can move a real vault
            // somewhere committed code cannot follow.
            if let source = Bundle.main.object(forInfoDictionaryKey: "OHHiveSourceCommit") as? String,
               !source.isEmpty, source != "unknown" {
                Text(source)
                    .font(.caption2.monospaced())
                    .foregroundStyle(source.hasSuffix("-dirty") ? .orange : .secondary)
            }
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
