import XCTest
@testable import PullYard

final class AccountLockTests: XCTestCase {
    private var store: MemoryLockStore!
    private var lock: AccountLock!
    private var clock = Date(timeIntervalSince1970: 1_000_000)

    override func setUp() {
        store = MemoryLockStore()
        lock = AccountLock(store: store, iterations: 1_000)
        lock.now = { [unowned self] in clock }
    }

    func testValidatesPINFormat() {
        XCTAssertTrue(AccountLock.isValidPIN("1234"))
        XCTAssertTrue(AccountLock.isValidPIN("123456"))
        XCTAssertFalse(AccountLock.isValidPIN("123"))
        XCTAssertFalse(AccountLock.isValidPIN("1234567"))
        XCTAssertFalse(AccountLock.isValidPIN("12a4"))
        XCTAssertFalse(AccountLock.isValidPIN("١٢٣٤"))
    }

    func testStoresHashNotPIN() throws {
        lock.setPIN("1234", for: "a")
        let record = try XCTUnwrap(store.load("a"))
        XCTAssertFalse(record.hash.isEmpty)
        XCTAssertNotEqual(record.hash, Data("1234".utf8))
        XCTAssertEqual(record.salt.count, 16)
    }

    func testCorrectPINUnlocksAndResetsFailures() {
        lock.setPIN("1234", for: "a")
        XCTAssertEqual(lock.verify("0000", for: "a"), .wrong(attemptsLeft: 4))
        XCTAssertEqual(lock.verify("1234", for: "a"), .ok)
        XCTAssertEqual(store.load("a")?.failures, 0)
    }

    func testPausesAfterFiveFailuresAndGrows() {
        lock.setPIN("1234", for: "a")
        for _ in 0..<4 { _ = lock.verify("0000", for: "a") }
        XCTAssertEqual(lock.verify("0000", for: "a"), .wait(until: clock.addingTimeInterval(60)))
        XCTAssertEqual(lock.verify("1234", for: "a"), .wait(until: clock.addingTimeInterval(60)))
        clock = clock.addingTimeInterval(61)
        XCTAssertEqual(lock.verify("0000", for: "a"), .wait(until: clock.addingTimeInterval(120)))
    }

    func testBlocksAfterTenFailures() {
        lock.setPIN("1234", for: "a")
        lock.setBiometryState(Data([1]), for: "a")
        for _ in 0..<10 {
            _ = lock.verify("0000", for: "a")
            clock = clock.addingTimeInterval(3600)
        }
        XCTAssertEqual(lock.verify("1234", for: "a"), .blocked)
        XCTAssertFalse(lock.biometricsEnabled("a"))
        XCTAssertFalse(lock.biometryMatches(Data([1]), for: "a"))
    }

    func testBiometryChangeDisablesTouchID() {
        lock.setPIN("1234", for: "a")
        lock.setBiometryState(Data([1, 2]), for: "a")
        XCTAssertTrue(lock.biometryMatches(Data([1, 2]), for: "a"))
        XCTAssertFalse(lock.biometryMatches(Data([9]), for: "a"))
        XCTAssertFalse(lock.biometricsEnabled("a"))
        XCTAssertFalse(lock.biometryMatches(Data([1, 2]), for: "a"))
    }

    func testAccountsAreSeparate() {
        lock.setPIN("1111", for: "a")
        lock.setPIN("2222", for: "b")
        XCTAssertNotEqual(lock.verify("1111", for: "b"), .ok)
        XCTAssertEqual(lock.verify("2222", for: "b"), .ok)
        XCTAssertFalse(lock.isProtected("c"))
    }
}
