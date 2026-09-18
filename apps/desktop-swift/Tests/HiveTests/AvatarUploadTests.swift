import XCTest
import ImageIO
import UniformTypeIdentifiers
@testable import Hive

final class AvatarUploadTests: XCTestCase {
    func testUploadNormalizesNonSquareImageAndStripsMetadata() throws {
        let context = try XCTUnwrap(CGContext(data: nil, width: 640, height: 320, bitsPerComponent: 8, bytesPerRow: 640*4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        context.setFillColor(CGColor(red: 1, green: 0, blue: 0, alpha: 1)); context.fill(CGRect(x: 0,y: 0,width: 640,height: 320))
        let image = try XCTUnwrap(context.makeImage())
        let input = NSMutableData()
        let destination = try XCTUnwrap(CGImageDestinationCreateWithData(input, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination,image,[kCGImagePropertyTIFFDictionary: [kCGImagePropertyTIFFArtist: "Private source metadata"]] as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        let value = try AvatarUpload.normalize(input as Data)
        let bytes = try XCTUnwrap(Data(base64Encoded: String(value.dropFirst(AvatarUpload.prefix.count))))
        let source = try XCTUnwrap(CGImageSourceCreateWithData(bytes as CFData,nil))
        let output = try XCTUnwrap(CGImageSourceCreateImageAtIndex(source,0,nil))
        XCTAssertEqual(output.width,256); XCTAssertEqual(output.height,256)
        let props = try XCTUnwrap(CGImageSourceCopyPropertiesAtIndex(source,0,nil) as? [String:Any])
        XCTAssertNil(props[kCGImagePropertyTIFFDictionary as String])
        XCTAssertNotNil(AvatarUpload.image(value))
    }
    func testInvalidAndOversizedUploadsAreRejected() {
        XCTAssertThrowsError(try AvatarUpload.normalize(Data("not an image".utf8)))
        XCTAssertThrowsError(try AvatarUpload.normalize(Data(count: AvatarUpload.maximumBytes+1)))
        XCTAssertNil(AvatarUpload.image("https://example.com/image.png"))
    }
}
