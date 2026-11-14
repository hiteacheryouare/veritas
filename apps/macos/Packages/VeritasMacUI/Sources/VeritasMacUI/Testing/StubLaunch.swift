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
	private let budget = StubBudget()
	private let reports = StubReports()
	private let accountStore = StubAccounts()
	private lazy var dashboardStore = StubDashboards(reports: reports)

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
		BudgetOverview(name: name, directory: StubLaunch.budgetDirectory, readOnly: readOnly, accountGroups: name == "Stub" ? accountStore.sidebar() : accountGroups)
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

	func budgetMonth(_ month: Month, today: String) async throws -> BudgetMonth {
		budget.month(month)
	}

	func goals(today: String) async throws -> [GoalSummary] {
		budget.goals
	}

	func goalClosePreview(_ goalID: String, releaseTo: String?, today: String) async throws -> GoalClosePreview {
		budget.closePreview(goalID, releaseTo: releaseTo)
	}

	func categoryHistory(_ categoryID: String) async throws -> CategoryHistory {
		budget.history(categoryID)
	}

	func perform(_ command: Command) async throws {
		if let readOnly { throw FfiError.ReadOnly(reason: readOnly) }
		switch command {
		case .saveCustomReport(let settings): try reports.save(settings)
		case .placeWidget(let id, let x, let y, let width, let height): try dashboardStore.place(id, x: x, y: y, width: width, height: height)
		default:
			if try !accountStore.perform(command) { try budget.perform(command) }
		}
	}

	func accounts() async throws -> AccountList { accountStore.list() }
	func register(_ scope: RegisterScope, filter: RegisterFilter, sort: RegisterSort, offset: UInt64, limit: UInt64) async throws -> RegisterPage {
		accountStore.register(scope, filter: filter, sort: sort, offset: offset, limit: limit)
	}
	func registerRow(_ id: String) async throws -> RegisterRow { try accountStore.registerRow(id) }
	func payees() async throws -> [PayeeDetail] { accountStore.payeeList() }
	func payeeMemory(_ payeeID: String) async throws -> PayeeMemory? { accountStore.memory(payeeID) }
	func reconciliation(_ accountID: String, statementBalance: Int64) async throws -> Reconciliation {
		accountStore.reconciliation(accountID, statement: statementBalance)
	}

	func netProfit(first: Month, last: Month) async throws -> NetProfitReport { reports.netProfit(first: first, last: last) }
	func netWorth(first: Month, last: Month) async throws -> [NetWorthMonth] { reports.netWorth(first: first, last: last) }
	func cashFlow(first: Month, last: Month) async throws -> [CashFlowColumn] { reports.cashFlow(first: first, last: last) }
	func cashFlowStatement(first: String, last: String) async throws -> CashFlowColumn { reports.cashFlowStatement(first: first, last: last) }
	func sankey(first: String, last: String) async throws -> SankeyReport { reports.sankey(first: first, last: last) }
	func reportTransactions(_ drill: ReportDrill) async throws -> [ReportTransaction] { reports.transactions(drill) }
	func customReports() async throws -> [CustomReportItem] { reports.customReports() }
	func customReport(_ id: String, today: String) async throws -> CustomReportView { try reports.customReport(id) }
	func previewCustomReport(_ settings: CustomReportSettings, today: String) async throws -> CustomReportView { StubReports.view(settings) }
	func dashboards(today: String) async throws -> [DashboardPage] { [] }
	let canEditDashboards = true
	func dashboardSheets(today: String) async throws -> [DashboardSheet] { dashboardStore.all() }
	func addWidget(page: String, kind: WidgetKind, x: Int64, y: Int64, width: Int64, height: Int64, config: WidgetConfig) async throws -> String {
		try writable()
		return try dashboardStore.add(page: page, kind: kind, frame: (x, y, width, height), config: config)
	}
	func removeWidget(_ id: String) async throws {
		try writable()
		try dashboardStore.remove(id)
	}
	func setWidgetConfig(_ id: String, _ config: WidgetConfig) async throws {
		try writable()
		try dashboardStore.configure(id, config)
	}
	func createDashboard(named name: String) async throws -> String {
		try writable()
		return dashboardStore.createDashboard(name)
	}
	func renameDashboard(_ id: String, to name: String) async throws {
		try writable()
		dashboardStore.renameDashboard(id, name)
	}
	func deleteDashboard(_ id: String) async throws {
		try writable()
		dashboardStore.deleteDashboard(id)
	}
	func reportDraft(_ id: String) async throws -> ReportDraft { try reports.draft(id) }
	func previewReport(_ draft: ReportDraft, today: String) async throws -> CustomReportView { StubReports.view(draft.settings) }
	func saveReport(_ draft: ReportDraft) async throws {
		try writable()
		try reports.save(draft)
	}
	func createReport(_ draft: ReportDraft) async throws -> String {
		try writable()
		return try reports.create(draft)
	}

	private func writable() throws {
		if let readOnly { throw FfiError.ReadOnly(reason: readOnly) }
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
