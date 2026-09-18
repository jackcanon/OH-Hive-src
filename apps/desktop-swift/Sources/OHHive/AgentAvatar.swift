import SwiftUI
import AppKit

struct AgentBiography: Codable, Equatable {
    var bio = ""
    var instructions = ""
    var avatar = ""
    var revision: UInt32 = 0
    var isValid: Bool { bio.utf8.count <= 4000 && instructions.utf8.count <= 16000 }
}

struct AgentAvatar: View {
    static let choices = ["baldr", "bragi", "eir", "forseti", "freyja", "freyr", "frigg", "heimdall", "hel", "hodr", "idunn", "loki", "njord", "odin", "sif", "skadi", "thor", "tyr", "ullr", "vali", "vidar"]
    private static let images: [String: NSImage] = Dictionary(uniqueKeysWithValues: choices.compactMap { name in
        guard let url = Bundle.module.url(forResource: name + "-128", withExtension: "gif", subdirectory: "Avatars"),
              let image = NSImage(contentsOf: url) else { return nil }
        return (name, image)
    })
    @State private var uploadedImage: NSImage?
    let name: String
    var size: CGFloat = 72
    var body: some View {
        Group {
            if let image = Self.images[name] ?? uploadedImage {
                Image(nsImage: image).resizable().scaledToFit()
            } else { Image(systemName: "person.crop.circle.fill").resizable().scaledToFit().foregroundStyle(.secondary) }
        }.frame(width: size, height: size).clipShape(.circle)
            .accessibilityLabel(name.hasPrefix(AvatarUpload.prefix) ? "Uploaded avatar" : (name.isEmpty ? "Default agent avatar" : name.capitalized + " avatar"))
            .task(id: name) { uploadedImage = AvatarUpload.image(name) }
    }
}
