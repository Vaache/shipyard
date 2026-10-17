import CommonCrypto
import Foundation
import LocalAuthentication
import Security

struct LockRecord: Codable, Equatable {
    var salt: Data
    var hash: Data
    var iterations: Int
    var failures = 0
    var lockedUntil: Date?
    var biometryState: Data?
}

protocol LockStore {
    func load(_ id: String) -> LockRecord?
    func save(_ record: LockRecord, _ id: String)
    func delete(_ id: String)
}

struct KeychainLockStore: LockStore {
    private static let service = "dev.vaache.shipyard.lock"

    private func base(_ id: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: Self.service,
         kSecAttrAccount as String: id]
    }

    func load(_ id: String) -> LockRecord? {
        var query = base(id)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: AnyObject?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess, let data = result as? Data else { return nil }
        return try? JSONDecoder().decode(LockRecord.self, from: data)
    }

    func save(_ record: LockRecord, _ id: String) {
        guard let data = try? JSONEncoder().encode(record) else { return }
        let update = [kSecValueData as String: data, kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly] as [String: Any]
        if SecItemUpdate(base(id) as CFDictionary, update as CFDictionary) == errSecItemNotFound {
            var query = base(id)
            query.merge(update) { $1 }
            SecItemAdd(query as CFDictionary, nil)
        }
    }

    func delete(_ id: String) {
        SecItemDelete(base(id) as CFDictionary)
    }
}

final class MemoryLockStore: LockStore {
    private var records: [String: LockRecord] = [:]
    func load(_ id: String) -> LockRecord? { records[id] }
    func save(_ record: LockRecord, _ id: String) { records[id] = record }
    func delete(_ id: String) { records[id] = nil }
}

enum PINCheck: Equatable {
    case ok
    case wrong(attemptsLeft: Int)
    case wait(until: Date)
    case blocked
}

final class AccountLock {
    static let shared = AccountLock(store: KeychainLockStore())
    static let pinLength = 4...6
    static let freeAttempts = 5
    static let maxAttempts = 10
    static let baseDelay: TimeInterval = 60
    static let maxDelay: TimeInterval = 15 * 60

    private let store: LockStore
    private let iterations: Int
    var now: () -> Date = Date.init

    init(store: LockStore, iterations: Int = 200_000) {
        self.store = store
        self.iterations = iterations
    }

    func isProtected(_ id: String) -> Bool { store.load(id) != nil }

    func biometricsEnabled(_ id: String) -> Bool { store.load(id)?.biometryState != nil }

    static func isValidPIN(_ pin: String) -> Bool {
        pinLength.contains(pin.count) && pin.allSatisfy(\.isASCII) && pin.allSatisfy(\.isNumber)
    }

    func setPIN(_ pin: String, for id: String) {
        var salt = Data(count: 16)
        _ = salt.withUnsafeMutableBytes { SecRandomCopyBytes(kSecRandomDefault, 16, $0.baseAddress!) }
        store.save(LockRecord(salt: salt, hash: derive(pin, salt: salt, iterations: iterations), iterations: iterations), id)
    }

    func removeLock(_ id: String) {
        store.delete(id)
    }

    func state(_ id: String) -> PINCheck? {
        guard let r = store.load(id) else { return nil }
        if r.failures >= Self.maxAttempts { return .blocked }
        if let until = r.lockedUntil, until > now() { return .wait(until: until) }
        return nil
    }

    func verify(_ pin: String, for id: String) -> PINCheck {
        guard var r = store.load(id) else { return .ok }
        if let blocked = state(id) { return blocked }
        if derive(pin, salt: r.salt, iterations: r.iterations) == r.hash {
            r.failures = 0
            r.lockedUntil = nil
            store.save(r, id)
            return .ok
        }
        r.failures += 1
        defer { store.save(r, id) }
        if r.failures >= Self.maxAttempts {
            r.biometryState = nil
            return .blocked
        }
        if r.failures >= Self.freeAttempts {
            let delay = min(Self.baseDelay * pow(2, Double(r.failures - Self.freeAttempts)), Self.maxDelay)
            let until = now().addingTimeInterval(delay)
            r.lockedUntil = until
            return .wait(until: until)
        }
        return .wrong(attemptsLeft: Self.freeAttempts - r.failures)
    }

    func setBiometryState(_ state: Data?, for id: String) {
        guard var r = store.load(id) else { return }
        r.biometryState = state
        store.save(r, id)
    }

    func biometryMatches(_ state: Data?, for id: String) -> Bool {
        guard let r = store.load(id), let saved = r.biometryState, let state else { return false }
        guard saved == state else {
            setBiometryState(nil, for: id)
            return false
        }
        if r.failures >= Self.maxAttempts { return false }
        var cleared = r
        cleared.failures = 0
        cleared.lockedUntil = nil
        store.save(cleared, id)
        return true
    }

    private func derive(_ pin: String, salt: Data, iterations: Int) -> Data {
        var out = Data(count: Int(CC_SHA256_DIGEST_LENGTH))
        let password = Array(pin.utf8).map { Int8(bitPattern: $0) }
        _ = out.withUnsafeMutableBytes { outBytes in
            salt.withUnsafeBytes { saltBytes in
                CCKeyDerivationPBKDF(
                    CCPBKDFAlgorithm(kCCPBKDF2),
                    password, password.count,
                    saltBytes.bindMemory(to: UInt8.self).baseAddress, salt.count,
                    CCPseudoRandomAlgorithm(kCCPRFHmacAlgSHA256),
                    UInt32(iterations),
                    outBytes.bindMemory(to: UInt8.self).baseAddress, Int(CC_SHA256_DIGEST_LENGTH)
                )
            }
        }
        return out
    }
}

enum SystemAuth {
    static var biometricsAvailable: Bool {
        LAContext().canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: nil)
    }

    static func biometrics(_ reason: String) async -> Data? {
        let context = LAContext()
        guard (try? await context.evaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, localizedReason: reason)) == true else { return nil }
        return state(context)
    }

    static func deviceOwner(_ reason: String) async -> Bool {
        (try? await LAContext().evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason)) == true
    }

    private static func state(_ context: LAContext) -> Data? {
        if #available(macOS 15, *) {
            return context.domainState.biometry.stateHash
        }
        return context.evaluatedPolicyDomainState
    }
}
