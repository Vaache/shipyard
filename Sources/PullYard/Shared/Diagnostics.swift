import Foundation

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
    let origin: RequestOrigin
    let durationMs: Double
    let bytes: Int
    let status: Int?
    let error: String?
    let rateLimitRemaining: Int?
    let rateLimitReset: Int?
    let rateLimitResource: String?
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
    nonisolated(unsafe) static private(set) var isEnabled = false

    struct Token {
        let start: UInt64
        let tick: Int?
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

    private static let tickIdleNs: UInt64 = 300_000_000
    private static let tickMinNs: UInt64 = 1_000_000_000
    private static let tickMaxNs: UInt64 = 30_000_000_000
    private static let slowUIThresholdMs = 16.0

    private let lock = NSLock()
    private var requests = RingBuffer<RequestRecord>(capacity: 1000)
    private var ticks = RingBuffer<TickRecord>(capacity: 200)
    private var slowUI = RingBuffer<SlowUIRecord>(capacity: 200)
    private var openTick: OpenTick?
    private var nextID = 0
    private var version = 0
    private var observer: CFRunLoopObserver?
    private var busyStart: UInt64 = 0

    private static func now() -> UInt64 {
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
        Self.isEnabled = on
        if on { installObserver() } else { removeObserver() }
        if !on {
            lock.lock()
            openTick = nil
            lock.unlock()
        }
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
        openTick = nil
        version += 1
        lock.unlock()
    }

    private func makeID() -> Int {
        nextID += 1
        return nextID
    }

    static func track(_ kind: RequestKind, _ name: @autoclosure () -> String, _ operation: () async throws -> (Data, URLResponse)) async throws -> (Data, URLResponse) {
        guard isEnabled else { return try await operation() }
        let token = shared.begin()
        do {
            let result = try await operation()
            shared.finish(token, kind: kind, name: name(), data: result.0, response: result.1, error: nil)
            return result
        } catch {
            shared.finish(token, kind: kind, name: name(), data: nil, response: nil, error: error)
            throw error
        }
    }

    static func restName(_ method: String, _ url: URL?) -> String {
        "\(method) \(url?.path ?? "?")"
    }

    static func graphQLName(_ query: String, caller: String) -> String {
        let scanner = Scanner(string: query)
        scanner.charactersToBeSkipped = .whitespacesAndNewlines
        if scanner.scanString("query") != nil || scanner.scanString("mutation") != nil || scanner.scanString("subscription") != nil,
           let name = scanner.scanCharacters(from: CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "_"))) {
            return name
        }
        return caller.split(separator: "(").first.map(String.init) ?? caller
    }

    private func begin() -> Token {
        let start = Self.now()
        lock.lock()
        defer { lock.unlock() }
        guard var tick = openTick else { return Token(start: start, tick: nil) }
        tick.inflight += 1
        tick.requests += 1
        tick.lastActivity = start
        openTick = tick
        return Token(start: start, tick: tick.id)
    }

    private func finish(_ token: Token, kind: RequestKind, name: String, data: Data?, response: URLResponse?, error: Error?) {
        let end = Self.now()
        let http = response as? HTTPURLResponse
        let errorText: String? = error.map { e in
            if e is CancellationError || (e as? URLError)?.code == .cancelled { return "cancelled" }
            return e.localizedDescription
        }
        lock.lock()
        defer { lock.unlock() }
        let record = RequestRecord(
            id: makeID(),
            date: Date(),
            kind: kind,
            name: name,
            origin: token.tick == nil ? .user : .liveTick,
            durationMs: Self.ms(token.start, end),
            bytes: data?.count ?? 0,
            status: http?.statusCode,
            error: errorText,
            rateLimitRemaining: http?.value(forHTTPHeaderField: "X-RateLimit-Remaining").flatMap { Int($0) },
            rateLimitReset: http?.value(forHTTPHeaderField: "X-RateLimit-Reset").flatMap { Int($0) },
            rateLimitResource: http?.value(forHTTPHeaderField: "X-RateLimit-Resource")
        )
        requests.append(record)
        version += 1
        if let id = token.tick, var tick = openTick, tick.id == id {
            tick.inflight -= 1
            tick.lastActivity = end
            tick.lastEnd = end
            openTick = tick
            if tick.inflight == 0 { scheduleTickCheck(after: Self.tickIdleNs) }
        }
    }

    @MainActor
    func beginTick() {
        guard Self.isEnabled else { return }
        let start = Self.now()
        lock.lock()
        closeTickLocked()
        openTick = OpenTick(id: makeID(), date: Date(), start: start, lastActivity: start)
        lock.unlock()
        scheduleTickCheck(after: Self.tickMinNs)
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
        if age >= Self.tickMaxNs || (tick.inflight == 0 && age >= Self.tickMinNs && idle >= Self.tickIdleNs) {
            closeTickLocked()
        } else if tick.inflight == 0 {
            let wait = max(Self.tickMinNs &- min(age, Self.tickMinNs), Self.tickIdleNs &- min(idle, Self.tickIdleNs))
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
    private func installObserver() {
        guard observer == nil else { return }
        let activities = CFRunLoopActivity.afterWaiting.rawValue | CFRunLoopActivity.beforeWaiting.rawValue
        let obs = CFRunLoopObserverCreateWithHandler(nil, activities, true, 0) { [weak self] _, activity in
            self?.runLoopActivity(activity)
        }
        observer = obs
        busyStart = 0
        CFRunLoopAddObserver(CFRunLoopGetMain(), obs, .commonModes)
    }

    @MainActor
    private func removeObserver() {
        guard let obs = observer else { return }
        CFRunLoopRemoveObserver(CFRunLoopGetMain(), obs, .commonModes)
        observer = nil
    }

    private func runLoopActivity(_ activity: CFRunLoopActivity) {
        if activity == .afterWaiting {
            busyStart = Self.now()
            return
        }
        guard busyStart != 0 else { return }
        let duration = Self.ms(busyStart, Self.now())
        busyStart = 0
        guard duration > Self.slowUIThresholdMs else { return }
        lock.lock()
        slowUI.append(SlowUIRecord(id: makeID(), date: Date(), durationMs: duration))
        version += 1
        lock.unlock()
    }
}
