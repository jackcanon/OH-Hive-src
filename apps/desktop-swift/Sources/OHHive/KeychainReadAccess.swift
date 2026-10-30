import Foundation
import Security

/// Compatibility for existing file-based Mac Keychain items. The modern SecItem
/// no-UI flag alone does not suppress legacy item ACL prompts on macOS.
enum KeychainReadAccess {
    private static let lock = NSRecursiveLock()

    static func perform<T>(allowInteraction: Bool, _ read: () -> T?) -> T? {
        lock.lock()
        defer { lock.unlock() }
        if allowInteraction { return read() }
        var previous = DarwinBoolean(false)
        guard SecKeychainGetUserInteractionAllowed(&previous) == errSecSuccess,
              SecKeychainSetUserInteractionAllowed(false) == errSecSuccess else { return nil }
        defer { _ = SecKeychainSetUserInteractionAllowed(previous.boolValue) }
        return read()
    }
}
