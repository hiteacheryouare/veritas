#if DEBUG
import Foundation
import VeritasCore
import VeritasKit

/// UI tests launch the app with `-VeritasStub empty` or `-VeritasStub open`, plus
/// optionally `-VeritasStubReadOnly <kind>`, `-VeritasStubImportError <kind>`,
/// `-VeritasStubFindings YES` or `-VeritasStubBackupProblem YES`. The stub keeps
/// the UI tests independent of fixtures, file panels and the sandbox.
struct StubLaunch {
	static let budgetDirectory = URL(filePath: "/stub/budget", directoryHint: .isDirectory)
	static let zip = URL(filePath: "/stub/Budget.zip")
	static let backupFolder = URL(filePath: "/stub/Backups", directoryHint: .isDirectory)

	static let current: StubLaunch? = StubLaunch(defaults: .standard)

	let opensBudgetOnLaunch: Bool
	let library: StubLibrary

	init?(defaults: UserDefaults) {
		guard let mode = defaults.string(forKey: "VeritasStub") else { return nil }
		opensBudgetOnLaunch = mode == "open"
		library = StubLibrary(
			readOnly: defaults.string(forKey: "VeritasStubReadOnly").flatMap(Self.readOnlyKind),
			importError: defaults.string(forKey: "VeritasStubImportError").flatMap(Self.error),
			hasFindings: defaults.bool(forKey: "VeritasStubFindings"),
			backupFails: defaults.bool(forKey: "VeritasStubBackupProblem")
		)
	}

	private static func readOnlyKind(_ name: String) -> ReadOnlyKind? {
		switch name {
		case "newerActualVersion": .newerActualVersion
		case "trackingBudget": .trackingBudget
		case "openElsewhere": .openElsewhere
		case "auditFailed": .auditFailed
		case "newerVeritasSchema": .newerVeritasSchema
		default: nil
		}
	}

	private static func error(_ name: String) -> FfiError? {
		switch name {
		case "upgradeInActualFirst": .UpgradeInActualFirst(required: "26.11.0")
		case "notAnActualBudget": .NotAnActualBudget(message: "The zip has no db.sqlite.")
		case "storage": .Storage(message: "disk I/O error")
		case "io": .Io(message: "Can't read /stub/Budget.zip. Check that the file still exists.")
		case "internal": .Internal(message: "stub bug")
		case "auditFailed": .AuditFailed(violations: 2)
		default: nil
		}
	}
}

struct StubLibrary: BudgetLibrary {
	let readOnly: ReadOnlyKind?
	let importError: FfiError?
	var hasFindings = false
	var backupFails = false

	func listBudgets() async throws -> [BudgetListingItem] {
		[
			BudgetListingItem(id: "household", name: "Household", folder: StubLaunch.budgetDirectory, lastOpened: Date(timeIntervalSince1970: 1_794_384_000)),
			BudgetListingItem(id: "untitled", name: nil, folder: StubLaunch.budgetDirectory, lastOpened: nil)
		]
	}

	func newBudgetDirectory() -> URL {
		StubLaunch.budgetDirectory
	}

	func open(directory: URL) async throws -> any BudgetSession {
		StubSession(readOnly: readOnly, hasFindings: hasFindings, backupFails: backupFails)
	}

	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession {
		if let importError { throw importError }
		return StubSession(readOnly: readOnly, hasFindings: hasFindings, backupFails: backupFails)
	}

	func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession {
		let accounts = budget.accounts.enumerated().map { index, account in
			(account.type.group, SidebarAccount(id: "new-\(index)", name: account.name, balance: account.isOwedBalance))
		}
		let groups = VeritasCore.AccountGroup.allCases.compactMap { group -> SidebarGroup? in
			let members = accounts.filter { $0.0 == group }.map(\.1)
			return members.isEmpty ? nil : SidebarGroup(name: group.title, accounts: members)
		}
		return StubSession(readOnly: readOnly, name: budget.name, groups: groups)
	}

	func currencies() async throws -> [CurrencyOption] {
		[
			CurrencyOption(code: "USD", name: "US Dollar", symbol: "$", numberStyle: .commaDot),
			CurrencyOption(code: "EUR", name: "Euro", symbol: "€", numberStyle: .dotComma),
			CurrencyOption(code: "CHF", name: "Swiss Franc", symbol: "Fr.", numberStyle: .apostropheDot)
		]
	}

	/// Plain decimals only. The real reader is the core's.
	func evaluateAmount(_ text: String, style: NumberStyle) async throws -> Int64 {
		let plain = text.replacing(",", with: "").trimmingCharacters(in: .whitespaces)
		guard plain.wholeMatch(of: /-?\d+(\.\d+)?/) != nil, let value = Decimal(string: plain) else {
			throw FfiError.Invalid(message: "Can't read \(text) as an amount. Fix it or clear it.")
		}
		return NSDecimalNumber(decimal: value * 100).int64Value
	}
}

final class StubSession: BudgetSession, @unchecked Sendable {
	let readOnly: ReadOnlyKind?
	let name: String
	let accountGroups: [SidebarGroup]
	let hasFindings: Bool
	let backupFails: Bool
	private let lock = NSLock()
	private var backup = BackupStatus.none
	private var snapshotList = [
		SnapshotInfo(id: "hourly", takenAt: "2026-11-11T15:00:00Z", reason: .hourly),
		SnapshotInfo(id: "open", takenAt: "2026-11-11T14:00:00Z", reason: .open)
	]
	private var keep = Retention(hourly: 48, daily: 30, monthly: 12, operationDays: 30)

	init(readOnly: ReadOnlyKind?, hasFindings: Bool = false, backupFails: Bool = false) {
		self.readOnly = readOnly
		self.hasFindings = hasFindings
		self.backupFails = backupFails
		name = "Stub"
		accountGroups = [
			SidebarGroup(name: "Cash", accounts: [SidebarAccount(id: "checking", name: "Checking", balance: 0)]),
			SidebarGroup(name: "Credit", accounts: [SidebarAccount(id: "card", name: "Card", balance: 0)])
		]
	}

	init(readOnly: ReadOnlyKind?, name: String, groups: [SidebarGroup]) {
		self.readOnly = readOnly
		self.name = name
		accountGroups = groups
		hasFindings = false
		backupFails = false
	}

	let canExport = true

	func overview() async throws -> BudgetOverview {
		BudgetOverview(name: name, directory: StubLaunch.budgetDirectory, readOnly: readOnly, accountGroups: accountGroups)
	}

	func undo() async throws {}

	func redo() async throws {}

	func maintain() async throws {}

	func exportActualZip(to destination: URL) async throws -> [AuditFinding] { [] }

	func observeChanges(_ changed: @escaping @Sendable () -> Void) async throws {}

	func close() async {}

	func backupStatus() async throws -> BackupStatus {
		lock.withLock { backup }
	}

	func setBackupFolder(_ folder: URL?) async throws {
		lock.withLock { backup = BackupStatus(folder: folder, lastAt: backup.lastAt, lastFile: backup.lastFile, lastProblem: backup.lastProblem) }
	}

	func backUpNow() async throws {
		let fails = backupFails
		lock.withLock {
			backup = BackupStatus(
				folder: backup.folder,
				lastAt: .now,
				lastFile: fails ? nil : "2026-11-11-Stub.zip",
				lastProblem: fails ? "Can't write to /stub/Backups. Check that the folder exists." : nil
			)
		}
	}

	func integrityFindings() async throws -> [AuditFinding]? {
		hasFindings ? [AuditFinding(check: .unpairedTransfer, detail: nil, table: "transactions", id: "t1", related: nil)] : []
	}

	func snapshots() async throws -> [SnapshotInfo] {
		lock.withLock { snapshotList }
	}

	func restoreSnapshot(id: String) async throws {
		lock.withLock { snapshotList.insert(SnapshotInfo(id: "restore", takenAt: "2026-11-11T16:00:00Z", reason: .restore), at: 0) }
	}

	func retention() async throws -> Retention {
		lock.withLock { keep }
	}

	func setRetention(_ retention: Retention) async throws {
		lock.withLock { keep = retention }
	}
}
#endif

#if DEBUG
private extension StartingAccount {
	/// Liabilities are negative, as the core stores them.
	var isOwedBalance: Int64 {
		type.isOwed ? -balance : balance
	}
}
#endif
