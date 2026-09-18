import Foundation
import ImageIO
import UniformTypeIdentifiers
import AppKit

/// Normalize a user-selected image without retaining source metadata or a local file path.
enum AvatarUpload {
    static let prefix = "data:image/png;base64,"
    static let maximumBytes = 10 * 1024 * 1024
    struct InvalidImage: LocalizedError {
        let errorDescription: String? = "Choose a PNG, JPEG, HEIC or GIF image up to 10 MB."
    }
    static func read(_ url: URL) throws -> String {
        let access = url.startAccessingSecurityScopedResource()
        defer { if access { url.stopAccessingSecurityScopedResource() } }
        let file = try FileHandle(forReadingFrom: url)
        defer { try? file.close() }
        let data = try file.read(upToCount: maximumBytes + 1) ?? Data()
        return try normalize(data)
    }
    static func normalize(_ data: Data) throws -> String {
        guard !data.isEmpty, data.count <= maximumBytes,
              let source = CGImageSourceCreateWithData(data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
              let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: 512,
                kCGImageSourceShouldCacheImmediately: true
              ] as CFDictionary),
              let context = CGContext(data: nil, width: 256, height: 256, bitsPerComponent: 8,
                  bytesPerRow: 256 * 4, space: CGColorSpaceCreateDeviceRGB(),
                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { throw InvalidImage() }
        let scale = max(256.0 / Double(image.width), 256.0 / Double(image.height))
        let width = Double(image.width) * scale, height = Double(image.height) * scale
        context.interpolationQuality = .high
        context.draw(image, in: CGRect(x: (256-width)/2, y: (256-height)/2, width: width, height: height))
        guard let result = context.makeImage() else { throw InvalidImage() }
        let output = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(output, UTType.png.identifier as CFString, 1, nil) else { throw InvalidImage() }
        CGImageDestinationAddImage(destination, result, nil)
        guard CGImageDestinationFinalize(destination), output.length < 384 * 1024 else { throw InvalidImage() }
        return prefix + (output as Data).base64EncodedString()
    }
    static func image(_ value: String) -> NSImage? {
        guard value.hasPrefix(prefix), value.utf8.count <= 512 * 1024,
              let bytes = Data(base64Encoded: String(value.dropFirst(prefix.count))) else { return nil }
        return NSImage(data: bytes)
    }
}
