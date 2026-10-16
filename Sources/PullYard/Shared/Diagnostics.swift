import AppKit
import Foundation
import os

enum RequestKind: String, Codable {
    case graphql, rest
}

enum RequestOrigin: String, Codable {
    case liveTick = "live-tick"
    case user
}

struct RequestRecord: Identifiable {
    let id: Int
    let date: Date
    let kind: RequestKind
    let name: String
    let path: String
    let origin: RequestOrigin
    let durationMs: Double
    let bytes: Int
    let status: Int?
    var error: String?
    var note: String?
    let authenticated: Bool
    let rateLimitRemaining: Int?
    let rateLimitReset: Int?
    let rateLimitResource: String?

    var failed: Bool { error != nil || (status ?? 200) >= 400 }

    var rateLimitBucket: String? {
        guard let resource = rateLimitResource else { return nil }
        return authenticated ? resource : resource + " (unauthenticated)"
    }
}

struct TickRecord: Identifiable {
    let id: Int
    let date: Date
    let durationMs: Double
    let requests: Int
}

struct SlowUIRecord: Identifiable {
    let id: Int
    let date: Date
    let durationMs: Double
}

struct DiagnosticsSnapshot {
    var version = 0
    var requests: [RequestRecord] = []
    var ticks: [TickRecord] = []
    var slowUI: [SlowUIRecord] = []
}

struct RingBuffer<Element> {
    let capacity: Int
    private(set) var storage: [Element] = []
    private var head = 0

    init(capacity: Int) {
        self.capacity = capacity
    }

    mutating func append(_ element: Element) {
        if storage.count < capacity {
            storage.append(element)
        } else {
            storage[head] = element
            head = (head + 1) % capacity
        }
    }

    mutating func update(where match: (Element) -> Bool, _ change: (inout Element) -> Void) {
        guard let i = storage.lastIndex(where: match) else { return }
        change(&storage[i])
    }

    mutating func removeAll() {
        storage = []
        head = 0
    }

    var newestFirst: [Element] {
        Array((storage[head...] + storage[..<head]).reversed())
    }
}

final class Diagnostics: @unchecked Sendable {
    static let enabledKey = "diagnosticsEnabled"
    static let shared = Diagnostics()
    @TaskLocal static var tick: Int?

    private static let enabled = OSAllocatedUnfairLock(initialState: false)

    static var isEnabled: Bool {
        enabled.withLock { $0 }
    }

    struct Token {
        let start: UInt64
        let date: Date
        let tick: Int?
        let generation: Int
    }

    private struct OpenTick {
        let id: Int
        let date: Date
        let start: UInt64
        var lastActivity: UInt64
        var lastEnd: UInt64?
        var inflight = 0
        var requests = 0
    }

    private static let tickSettleNs: UInt64 = 300_000_000
    private static let tickFirstRequestNs: UInt64 = 1_000_000_000
    private static let tickMaxNs: UInt64 = 30_000_000_000
    private static let slowUIThresholdMs = 16.0

    private let lock = NSLock()
    private var requests = RingBuffer<RequestRecord>(capacity: 1000)
    private var ticks = RingBuffer<TickRecord>(capacity: 200)
    private var slowUI = RingBuffer<SlowUIRecord>(capacity: 200)
    private var openTick: OpenTick?
    private var nextID = 0
    private var version = 0
    private var generation = 0
    private var observers: [CFRunLoopObserver] = []
    private var busyStart: UInt64 = 0
    private var modalInIteration = false
    private static let observedModes: [CFRunLoopMode] = [.commonModes, CFRunLoopMode(RunLoop.Mode.modalPanel.rawValue as CFString)]
    private var panelRefreshInIteration = false

    static func now() -> UInt64 {
        DispatchTime.now().uptimeNanoseconds
    }

    private static func ms(_ from: UInt64, _ to: UInt64) -> Double {
        Double(to &- from) / 1_000_000
    }

    @MainActor
    func start() {
        setEnabled(UserDefaults.standard.bool(forKey: Self.enabledKey))
    }

    @MainActor
    func setEnabled(_ on: Bool) {
        lock.lock()
        Self.enabled.withLock { $0 = on }
        generation += 1
        openTick = nil
        lock.unlock()
        if on { installObservers() } else { removeObservers() }
    }

    func snapshot(ifNewerThan known: Int) -> DiagnosticsSnapshot? {
        lock.lock()
        defer { lock.unlock() }
        guard version != known else { return nil }
        return DiagnosticsSnapshot(version: version, requests: requests.newestFirst, ticks: ticks.newestFirst, slowUI: slowUI.newestFirst)
    }

    func clear() {
        lock.lock()
        requests.removeAll()
        ticks.removeAll()
        slowUI.removeAll()
        generation += 1
        version += 1
        lock.unlock()
    }

    private func makeID() -> Int {
        nextID += 1
        return nextID
    }

    static func track(_ kind: RequestKind, _ name: @autoclosure () -> String, url: @autoclosure () -> URL?, authenticated: Bool = true, _ operation: () async throws -> (Data, URLResponse)) async throws -> (Data, URLResponse) {
        let (data, response, _) = try await measure(kind, name(), url: url(), authenticated: authenticated, operation)
        return (data, response)
    }

    static func measure(_ kind: RequestKind, _ name: @autoclosure () -> String, url: @autoclosure () -> URL?, authenticated: Bool = true, _ operation: () async throws -> (Data, URLResponse)) async throws -> (Data, URLResponse, Int?) {
        guard isEnabled else {
            let (data, response) = try await operation()
            return (data, response, nil)
        }
        let token = shared.begin()
        do {
            let (data, response) = try await operation()
            let id = shared.finish(token, kind: kind, name: name(), url: url(), authenticated: authenticated, data: data, response: response, error: nil)
            return (data, response, id)
        } catch {
            shared.finish(token, kind: kind, name: name(), url: url(), authenticated: authenticated, data: nil, response: nil, error: error)
            throw error
        }
    }

    func annotate(_ id: Int, note: String? = nil, error: String? = nil) {
        lock.lock()
        defer { lock.unlock() }
        requests.update(where: { $0.id == id }) { r in
            if let note { r.note = note }
            if let error { r.error = error }
        }
        version += 1
    }

    static func graphQLName(_ query: String) -> String {
        let scanner = Scanner(string: query)
        scanner.charactersToBeSkipped = .whitespacesAndNewlines
        if scanner.scanString("query") != nil || scanner.scanString("mutation") != nil || scanner.scanString("subscription") != nil,
           let name = scanner.scanCharacters(from: CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "_"))) {
            return name
        }
        return "anonymous"
    }

    static func restName(_ method: String, _ url: URL?) -> String {
        "\(method) \(normalizedPath(url?.path ?? "/"))"
    }

    static func normalizedPath(_ path: String) -> String {
        var parts = path.split(separator: "/").map(String.init)
        if parts.first == "repos", parts.count >= 3 {
            parts[1] = "{owner}"
            parts[2] = "{repo}"
            let rest = Array(parts.dropFirst(3))
            let collapse: [([String], String)] = [(["contents"], "{path}"), (["readme"], "{path}"), (["git", "refs", "heads"], "{branch}"), (["git", "trees"], "{sha}"), (["commits"], "{sha}")]
            for (prefix, placeholder) in collapse where rest.starts(with: prefix) && rest.count > prefix.count {
                parts = Array(parts.prefix(3)) + prefix + [placeholder] + (prefix == ["commits"] || prefix == ["git", "trees"] ? Array(rest.dropFirst(prefix.count + 1)) : [])
                break
            }
        }
        parts = parts.map { p in
            if p.allSatisfy(\.isNumber) { return "{n}" }
            if p.count == 40, p.allSatisfy(\.isHexDigit) { return "{sha}" }
            return p
        }
        return "/" + parts.joined(separator: "/")
    }

    static func errorText(_ e: Error) -> String {
        if e is CancellationError { return "cancelled" }
        if let u = e as? URLError {
            switch u.code {
            case .cancelled: return "cancelled"
            case .timedOut: return "timeout"
            default: return "URLError \(u.code.rawValue)"
            }
        }
        return e.localizedDescription
    }

    @MainActor
    @discardableResult
    static func liveTask(_ operation: @escaping @MainActor () async -> Void) -> Task<Void, Never> {
        guard isEnabled, let id = shared.currentTick() else { return Task { await operation() } }
        return $tick.withValue(id) { Task { await operation() } }
    }

    private func currentTick() -> Int? {
        lock.lock()
        defer { lock.unlock() }
        return openTick?.id
    }

    private func begin() -> Token {
        let start = Self.now()
        let date = Date()
        let tickID = Self.tick
        lock.lock()
        defer { lock.unlock() }
        if let tickID, var tick = openTick, tick.id == tickID {
            tick.inflight += 1
            tick.requests += 1
            tick.lastActivity = start
            openTick = tick
        }
        return Token(start: start, date: date, tick: tickID, generation: generation)
    }

    @discardableResult
    private func finish(_ token: Token, kind: RequestKind, name: String, url: URL?, authenticated: Bool, data: Data?, response: URLResponse?, error: Error?) -> Int? {
        let end = Self.now()
        let http = response as? HTTPURLResponse
        lock.lock()
        defer { lock.unlock() }
        if let id = token.tick, var tick = openTick, tick.id == id {
            tick.inflight -= 1
            tick.lastActivity = end
            tick.lastEnd = end
            openTick = tick
            if tick.inflight == 0 { scheduleTickCheck(after: Self.tickSettleNs) }
        }
        guard Self.isEnabled, token.generation == generation else { return nil }
        let record = RequestRecord(
            id: makeID(),
            date: token.date,
            kind: kind,
            name: name,
            path: url?.path ?? "",
            origin: token.tick == nil ? .user : .liveTick,
            durationMs: Self.ms(token.start, end),
            bytes: data?.count ?? 0,
            status: http?.statusCode,
            error: error.map(Self.errorText),
            note: nil,
            authenticated: authenticated,
            rateLimitRemaining: http?.value(forHTTPHeaderField: "X-RateLimit-Remaining").flatMap { Int($0) },
            rateLimitReset: http?.value(forHTTPHeaderField: "X-RateLimit-Reset").flatMap { Int($0) },
            rateLimitResource: http?.value(forHTTPHeaderField: "X-RateLimit-Resource")
        )
        requests.append(record)
        version += 1
        return record.id
    }

    @MainActor
    func beginTick() {
        guard Self.isEnabled else { return }
        let start = Self.now()
        lock.lock()
        closeTickLocked()
        openTick = OpenTick(id: makeID(), date: Date(), start: start, lastActivity: start)
        lock.unlock()
        scheduleTickCheck(after: Self.tickSettleNs)
    }

    private func scheduleTickCheck(after ns: UInt64) {
        DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + .nanoseconds(Int(ns))) { [weak self] in
            self?.checkTick()
        }
    }

    private func checkTick() {
        let now = Self.now()
        lock.lock()
        defer { lock.unlock() }
        guard let tick = openTick else { return }
        let age = now &- tick.start
        let idle = now &- tick.lastActivity
        let waitingForFirst = tick.requests == 0 && age < Self.tickFirstRequestNs
        if age >= Self.tickMaxNs || (tick.inflight == 0 && idle >= Self.tickSettleNs && !waitingForFirst) {
            closeTickLocked()
        } else if tick.inflight == 0 {
            let wait = waitingForFirst ? Self.tickFirstRequestNs &- age : Self.tickSettleNs &- min(idle, Self.tickSettleNs)
            scheduleTickCheck(after: max(wait, 10_000_000))
        } else {
            scheduleTickCheck(after: Self.tickMaxNs &- age)
        }
    }

    private func closeTickLocked() {
        guard let tick = openTick else { return }
        openTick = nil
        let duration = tick.lastEnd.map { Self.ms(tick.start, $0) } ?? 0
        ticks.append(TickRecord(id: makeID(), date: tick.date, durationMs: duration, requests: tick.requests))
        version += 1
    }

    @MainActor
    func markPanelRefresh() {
        panelRefreshInIteration = true
    }

    @MainActor
    private func installObservers() {
        guard observers.isEmpty else { return }
        let wake = CFRunLoopObserverCreateWithHandler(nil, CFRunLoopActivity.afterWaiting.rawValue, true, 0) { [weak self] _, _ in
            self?.iterationStarted()
        }
        let sleep = CFRunLoopObserverCreateWithHandler(nil, CFRunLoopActivity.beforeWaiting.rawValue, true, CFIndex.max) { [weak self] _, _ in
            self?.iterationEnded()
        }
        let entry = CFRunLoopObserverCreateWithHandler(nil, CFRunLoopActivity.entry.rawValue, true, 0) { [weak self] _, _ in
            self?.loopEntered()
        }
        busyStart = 0
        modalInIteration = false
        for obs in [wake, sleep, entry].compactMap({ $0 }) {
            for mode in Self.observedModes { CFRunLoopAddObserver(CFRunLoopGetMain(), obs, mode) }
            observers.append(obs)
        }
    }

    @MainActor
    private func removeObservers() {
        for obs in observers {
            for mode in Self.observedModes { CFRunLoopRemoveObserver(CFRunLoopGetMain(), obs, mode) }
        }
        observers = []
    }

    private static var inModalLoop: Bool {
        CFRunLoopCopyCurrentMode(CFRunLoopGetMain()).map { $0.rawValue as String == RunLoop.Mode.modalPanel.rawValue } ?? false
    }

    private func loopEntered() {
        if Self.inModalLoop { modalInIteration = true }
    }

    private func iterationStarted() {
        guard !Self.inModalLoop else { return }
        busyStart = Self.now()
        modalInIteration = false
        panelRefreshInIteration = false
    }

    private func iterationEnded() {
        guard !Self.inModalLoop, busyStart != 0 else { return }
        let duration = Self.ms(busyStart, Self.now())
        busyStart = 0
        guard !modalInIteration else {
            modalInIteration = false
            return
        }
        guard duration > Self.slowUIThresholdMs else { return }
        lock.lock()
        slowUI.append(SlowUIRecord(id: makeID(), date: Date(), durationMs: duration))
        if !panelRefreshInIteration { version += 1 }
        lock.unlock()
    }
}
