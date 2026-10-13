import Foundation
import Observation

enum AppLanguage: String, CaseIterable, Identifiable {
    case ru, en

    var id: String { rawValue }

    var title: String {
        switch self {
        case .ru: return "Русский"
        case .en: return "English"
        }
    }

    var locale: Locale { Locale(identifier: self == .ru ? "ru_RU" : "en_US") }
}

@Observable
final class Loc {
    static let shared = Loc()

    private(set) var lang: AppLanguage

    private init() {
        if let raw = UserDefaults.standard.string(forKey: "appLanguage"), let l = AppLanguage(rawValue: raw) {
            lang = l
        } else {
            lang = Locale.preferredLanguages.first?.hasPrefix("ru") == true ? .ru : .en
        }
    }

    func set(_ l: AppLanguage) {
        lang = l
        UserDefaults.standard.set(l.rawValue, forKey: "appLanguage")
    }
}

func t(_ ru: String, _ en: String) -> String {
    Loc.shared.lang == .ru ? ru : en
}

func tp(_ n: Int, _ ruOne: String, _ ruFew: String, _ ruMany: String, _ enOne: String, _ enMany: String) -> String {
    if Loc.shared.lang == .en { return "\(n) \(n == 1 ? enOne : enMany)" }
    let m10 = n % 10, m100 = n % 100
    if m10 == 1 && m100 != 11 { return "\(n) \(ruOne)" }
    if (2...4).contains(m10) && !(12...14).contains(m100) { return "\(n) \(ruFew)" }
    return "\(n) \(ruMany)"
}
