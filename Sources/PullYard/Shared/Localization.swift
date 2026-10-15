import Foundation

func plural(_ n: Int, _ one: String, _ many: String) -> String {
    "\(n) \(n == 1 ? one : many)"
}

extension Locale {
    static let app = Locale(identifier: "en_US")
}
