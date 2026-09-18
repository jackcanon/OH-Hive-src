import Foundation

/// Store intent only after successful sharing; an explicit stop survives relaunch.
struct FleetSharingPreferences {
    private let defaults: UserDefaults
    static let key = "privateFleetSharingAddress"
    init(defaults: UserDefaults = .standard) { self.defaults = defaults }
    var address: String? { defaults.string(forKey: Self.key) }
    func remember(address: String) { defaults.set(address, forKey: Self.key) }
    func stop() { defaults.removeObject(forKey: Self.key) }
}
