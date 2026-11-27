import Foundation
import VeritasCore

final class CoreBudgetSession: BudgetSession {
	private let session: Session
	private let listener = ChangeRelay()

	init(_ session: Session) {
		self.session = session
	}

	let canExport = true

	func overview() async throws -> BudgetOverview {
		let session = session
		let (state, overview, currencies) = try await Core.run { (try session.state(), try session.overview(), try VeritasCore.currencies()) }
		let currency = overview.numberFormat?.currency.flatMap { code in currencies.first { $0.code == code } }
		return BudgetOverview(
			name: overview.budgetName,
			directory: URL(filePath: state.directory, directoryHint: .isDirectory),
			readOnly: state.readOnly,
			canUndo: state.canUndo,
			canRedo: state.canRedo,
			isChecking: state.checking,
			accountGroups: overview.groups.filter { !$0.accounts.isEmpty }.map { group in
				SidebarGroup(name: group.group.title, accounts: group.accounts.map { account in
					SidebarAccount(id: account.id, name: account.name, balance: account.balance, isInactive: account.closed || account.deleted)
				})
			},
			format: overview.numberFormat.map { MoneyFormat($0, currency: currency) },
			currencyCode: currency.flatMap { $0.code.isEmpty ? nil : $0.code },
			numberStyle: overview.numberFormat?.style ?? .commaDot,
			scheduleProblem: state.scheduleProblem,
			snapshotProblem: state.snapshotProblem
		)
	}

	func undo() async throws {
		let session = session
		_ = try await Core.run { try session.undo() }
	}

	func redo() async throws {
		let session = session
		_ = try await Core.run { try session.redo() }
	}

	func maintain() async throws {
		let session = session
		let today = WorkspaceModel.today()
		_ = try await Core.run { try session.maintain(today: today) }
	}

	/// In the sandbox the core writes only in the app's own folders (contract v2), so
	/// the zip is made in the temporary directory, then moved onto the file the user chose.
	func exportActualZip(to destination: URL) async throws -> [AuditFinding] {
		let files = FileManager.default
		let staging = files.temporaryDirectory.appending(path: "export-\(UUID().uuidString)", directoryHint: .isDirectory)
		defer { try? files.removeItem(at: staging) }
		let staged = staging.appending(path: destination.lastPathComponent)
		do {
			try files.createDirectory(at: staging, withIntermediateDirectories: true)
		} catch {
			throw FfiError.Io(message: "Can't prepare the export in \(staging.path(percentEncoded: false)). Check the disk has space.")
		}
		let session = session
		let findings = try await Core.cancellable { try await session.exportActualZip(to: staged.path(percentEncoded: false), cancel: $0) }
		do {
			try Self.place(staged, at: destination)
		} catch {
			throw FfiError.Io(message: "Can't write \(destination.path(percentEncoded: false)). Check you can save files in that folder.")
		}
		return findings
	}

	/// Copies onto the destination's volume first, so the final replace is atomic there.
	private static func place(_ file: URL, at destination: URL) throws {
		let files = FileManager.default
		let nearby = try files.url(for: .itemReplacementDirectory, in: .userDomainMask, appropriateFor: destination, create: true)
		defer { try? files.removeItem(at: nearby) }
		let copy = nearby.appending(path: destination.lastPathComponent)
		try files.copyItem(at: file, to: copy)
		if files.fileExists(atPath: destination.path(percentEncoded: false)) {
			_ = try files.replaceItemAt(destination, withItemAt: copy)
		} else {
			try files.moveItem(at: copy, to: destination)
		}
	}

	func observeChanges(_ changed: @escaping @Sendable () -> Void) async throws {
		listener.set(changed)
		let session = session
		let listener = listener
		_ = try await Core.run { try session.subscribe(listener: listener) }
	}

	func close() async {
		let session = session
		_ = try? await Core.run { try session.close() }
	}

	func backupStatus() async throws -> BackupStatus {
		let session = session
		let state = try await Core.run { try session.state() }
		return BackupStatus(
			folder: state.backupFolder.map { URL(filePath: $0, directoryHint: .isDirectory) },
			lastAt: CoreTime.date(state.lastBackup?.at),
			lastFile: state.lastBackup?.file,
			lastProblem: state.lastBackup?.problem
		)
	}

	func setBackupFolder(_ folder: URL?) async throws {
		let session = session
		let path = folder?.path(percentEncoded: false)
		try await Core.run { try session.setBackupFolder(folder: path) }
	}

	func backUpNow() async throws {
		let session = session
		_ = try await Core.run { try session.backUpNow() }
	}

	func integrityFindings() async throws -> [AuditFinding]? {
		let session = session
		return try await Core.run { try session.auditReport() }
	}

	func snapshots() async throws -> [SnapshotInfo] {
		let session = session
		return try await Core.run { try session.snapshots() }
	}

	func restoreSnapshot(id: String) async throws {
		let session = session
		_ = try await Core.run { try session.restoreSnapshot(id: id) }
	}

	func retention() async throws -> Retention {
		let session = session
		return try await Core.run { try session.retention() }
	}

	func budgetMonth(_ month: Month, today: String) async throws -> BudgetMonth {
		let session = session
		return try await Core.run { try session.budgetMonth(month: month, today: today) }
	}

	func goals(today: String) async throws -> GoalList {
		let session = session
		return try await Core.run { try session.goals(today: today) }
	}

	func goalClosePreview(_ goalID: String, releaseTo: String?, today: String) async throws -> GoalClosePreview {
		let session = session
		return try await Core.run { try session.goalClosePreview(goalId: goalID, releaseToCategoryId: releaseTo, today: today) }
	}

	func categoryHistory(_ categoryID: String) async throws -> CategoryHistory {
		let session = session
		return try await Core.run { try session.categoryHistory(categoryId: categoryID) }
	}

	func netProfit(first: Month, last: Month) async throws -> NetProfitReport {
		let session = session
		return try await Core.cancellable { try await session.netProfit(first: first, last: last, cancel: $0) }
	}

	func netWorth(first: Month, last: Month) async throws -> [NetWorthMonth] {
		let session = session
		return try await Core.cancellable { try await session.netWorth(first: first, last: last, cancel: $0) }
	}

	func cashFlow(first: Month, last: Month) async throws -> [CashFlowColumn] {
		let session = session
		return try await Core.cancellable { try await session.cashFlow(first: first, last: last, cancel: $0) }
	}

	func budgetHabits(today: String) async throws -> BudgetHabits {
		let session = session
		return try await Core.run { try session.budgetHabits(today: today) }
	}

	func spendingByCategory(first: String, last: String) async throws -> CategoryBreakdown {
		let session = session
		return try await Core.cancellable { try await session.spendingByCategory(first: first, last: last, cancel: $0) }
	}

	func netProfitPeriod(first: String, last: String) async throws -> NetProfitPeriod {
		let session = session
		return try await Core.cancellable { try await session.netProfitPeriod(first: first, last: last, cancel: $0) }
	}

	func cashFlowStatement(first: String, last: String) async throws -> CashFlowColumn {
		let session = session
		return try await Core.cancellable { try await session.cashFlowStatement(first: first, last: last, cancel: $0) }
	}

	func sankey(first: String, last: String) async throws -> SankeyReport {
		let session = session
		return try await Core.cancellable { try await session.sankey(first: first, last: last, cancel: $0) }
	}

	func reportTransactions(_ drill: ReportDrill) async throws -> [ReportTransaction] {
		let session = session
		return try await Core.cancellable { try await session.reportTransactions(drill: drill, cancel: $0) }
	}

	func customReports() async throws -> [CustomReportItem] {
		let session = session
		return try await Core.run { try session.customReports() }
	}

	func customReport(_ id: String, today: String) async throws -> CustomReportView {
		let session = session
		return try await Core.cancellable { try await session.customReport(id: id, today: today, cancel: $0) }
	}

	func previewCustomReport(_ settings: CustomReportSettings, today: String) async throws -> CustomReportView {
		let session = session
		return try await Core.cancellable { try await session.previewCustomReport(settings: settings, today: today, cancel: $0) }
	}

	func dashboards(today: String) async throws -> [DashboardPage] {
		let session = session
		return try await Core.cancellable { try await session.dashboards(today: today, cancel: $0) }
	}

	func accounts() async throws -> AccountList {
		let session = session
		return try await Core.run { try session.accounts() }
	}

	func register(_ scope: RegisterScope, filter: RegisterFilter, sort: RegisterSort, offset: UInt64, limit: UInt64) async throws -> RegisterPage {
		let session = session
		return try await Core.run { try session.register(scope: scope, filter: filter, sort: sort, offset: offset, limit: limit) }
	}

	func registerRow(_ id: String) async throws -> RegisterRow {
		let session = session
		return try await Core.run { try session.registerRow(transactionId: id) }
	}

	func payees() async throws -> [PayeeDetail] {
		let session = session
		return try await Core.run { try session.payees() }
	}

	func payeeMemory(_ payeeID: String) async throws -> PayeeMemory? {
		let session = session
		return try await Core.run { try session.payeeMemory(payeeId: payeeID) }
	}

	func reconciliation(_ accountID: String, statementBalance: Int64) async throws -> Reconciliation {
		let session = session
		return try await Core.run { try session.reconciliation(accountId: accountID, statementBalance: statementBalance) }
	}

	/// Contract v9: Move money, category kinds and loans.
	let canPlanTransfers = true

	func transferPreview(from: String, to: String, amount: Int64) async throws -> TransferPreview {
		let session = session
		return try await Core.run { try session.transferPreview(fromAccountId: from, toAccountId: to, amount: amount) }
	}

	func kindTarget(_ kind: CategoryKind) async throws -> KindTarget {
		let session = session
		return try await Core.run { try session.kindTarget(kind: kind) }
	}

	func kindCategoryPlan(_ kind: CategoryKind) async throws -> KindCategoryPlan {
		let session = session
		return try await Core.run { try session.kindCategoryPlan(kind: kind) }
	}

	func kindChangePreview(_ categoryID: String, kind: CategoryKind) async throws -> UInt64 {
		let session = session
		return try await Core.run { try session.kindChangePreview(categoryId: categoryID, kind: kind) }
	}

	func setCategoryKind(_ categoryID: String, kind: CategoryKind) async throws {
		try await perform(.setCategoryKind(categoryId: categoryID, kind: kind))
	}

	func createKindCategory(kind: CategoryKind, name: String, unhideGroup: Bool) async throws -> String {
		try await created(.category, by: .createKindCategory(kind: kind, name: name, unhideGroup: unhideGroup))
	}

	func loanTerms(_ accountID: String) async throws -> LoanTerms? {
		let session = session
		return try await Core.run { try session.loanTerms(accountId: accountID) }
	}

	func setLoanTerms(_ accountID: String, _ terms: LoanTerms) async throws {
		try await perform(.setLoanTerms(accountId: accountID, terms: terms))
	}

	func loanPaymentDefaults(_ loanAccountID: String, today: String) async throws -> LoanPaymentDefaults {
		let session = session
		return try await Core.run { try session.loanPaymentDefaults(loanAccountId: loanAccountID, today: today) }
	}

	func loanPaymentPreview(_ loanAccountID: String, date: String, total: Int64, interest: Int64, escrow: Int64, fees: Int64) async throws -> LoanPaymentPreview {
		let session = session
		return try await Core.run {
			try session.loanPaymentPreview(loanAccountId: loanAccountID, date: date, total: total, interest: interest, escrow: escrow, fees: fees)
		}
	}

	func recordLoanPayment(_ payment: LoanPaymentDraft) async throws {
		try await perform(.recordLoanPayment(payment: payment))
	}

	func evaluateRate(_ text: String, style: NumberStyle) async throws -> Int64 {
		try await Core.run { try VeritasCore.evaluateRate(text: text, style: style) }
	}

	func balanceHistory(_ accountID: String, from: String, to: String) async throws -> BalanceHistory {
		let session = session
		return try await Core.run { try session.balanceHistory(accountId: accountID, from: from, to: to) }
	}

	func tags() async throws -> [TagSummary] {
		let session = session
		return try await Core.run { try session.tags() }
	}

	func createAccount(_ account: NewAccount, loan: LoanTerms?) async throws -> String {
		var account = account
		account.loan = loan
		return try await created(.account, by: .createAccount(account: account))
	}

	/// Contract v7: dashboards, widgets and report filters are editable.
	let canEditDashboards = true

	func dashboardSheets(today: String) async throws -> [DashboardSheet] {
		try await dashboards(today: today).map(DashboardSheet.init)
	}

	func addWidget(page: String, kind: WidgetKind, x: Int64, y: Int64, width: Int64, height: Int64, config: WidgetConfig) async throws -> String {
		try await created(.widget, by: .addWidget(pageId: page, kind: kind.rawValue, x: x, y: y, width: width, height: height, config: config))
	}

	func removeWidget(_ id: String) async throws { try await perform(.removeWidget(widgetId: id)) }
	func setWidgetConfig(_ id: String, _ config: WidgetConfig) async throws { try await perform(.setWidgetConfig(widgetId: id, config: config)) }
	func createDashboard(named name: String) async throws -> String { try await created(.dashboardPage, by: .createDashboard(name: name)) }
	func renameDashboard(_ id: String, to name: String) async throws { try await perform(.renameDashboard(pageId: id, name: name)) }
	func deleteDashboard(_ id: String) async throws { try await perform(.deleteDashboard(pageId: id)) }
	func createReport(_ draft: ReportDraft) async throws -> String { try await created(.customReport, by: .createCustomReport(settings: draft.settings)) }

	func reportDraft(_ id: String) async throws -> ReportDraft {
		ReportDraft(settings: try await customReport(id, today: WorkspaceModel.today()).settings)
	}

	func previewReport(_ draft: ReportDraft, today: String) async throws -> CustomReportView {
		try await previewCustomReport(draft.settings, today: today)
	}

	func saveReport(_ draft: ReportDraft) async throws {
		try await perform(.saveCustomReport(settings: draft.settings))
	}

	func bills(today: String) async throws -> [BillSummary] {
		let session = session
		return try await Core.run { try session.bills(today: today) }
	}

	func bill(_ id: String, today: String) async throws -> BillDetail {
		let session = session
		return try await Core.run { try session.bill(billId: id, today: today) }
	}

	func upcoming(days: UInt32, today: String) async throws -> [UpcomingBill] {
		let session = session
		return try await Core.run { try session.upcoming(days: days, today: today) }
	}

	func rules() async throws -> [RuleSentence] {
		let session = session
		return try await Core.run { try session.rules() }
	}

	func rulePreview(_ source: RuleSource) async throws -> [RuleMatch] {
		let session = session
		return try await Core.cancellable { try await session.rulePreview(source: source, cancel: $0) }
	}

	func statementEntity() async throws -> StatementEntity {
		let session = session
		return try await Core.run { try session.statementEntity() }
	}

	func incomeStatement(first: String, last: String) async throws -> IncomeStatement {
		let session = session
		return try await Core.cancellable { try await session.incomeStatement(first: first, last: last, cancel: $0) }
	}

	func wrapped(year: Int32, today: String) async throws -> WrappedReport {
		let session = session
		return try await Core.cancellable { try await session.wrapped(year: year, today: today, cancel: $0) }
	}

	func wrappedYears(today: String) async throws -> [Int32] {
		let session = session
		return try await Core.cancellable { try await session.wrappedYears(today: today, cancel: $0) }
	}

	func balanceSheet(asOf: String) async throws -> BalanceSheet {
		let session = session
		return try await Core.cancellable { try await session.balanceSheet(asOf: asOf, cancel: $0) }
	}

	func ownersEquity(first: String, last: String) async throws -> OwnersEquity {
		let session = session
		return try await Core.cancellable { try await session.ownersEquity(first: first, last: last, cancel: $0) }
	}

	func taxSummary(year: Int32) async throws -> TaxSummary {
		let session = session
		return try await Core.cancellable { try await session.taxSummary(year: year, cancel: $0) }
	}

	func forecast(today: String, scenarioIDs: [String]) async throws -> Forecast {
		let session = session
		return try await Core.cancellable { try await session.forecast(today: today, scenarioIds: scenarioIDs, cancel: $0) }
	}

	func budgetSuggestions(today: String) async throws -> SuggestionList {
		let session = session
		return try await Core.cancellable { try await session.budgetSuggestions(today: today, cancel: $0) }
	}

	func scenarios(today: String) async throws -> [ScenarioSummary] {
		let session = session
		return try await Core.run { try session.scenarios(today: today) }
	}

	func createScenario(named name: String, firstEvent: ScenarioChange?, today: String) async throws -> String {
		try await created(.scenario, by: .createScenario(name: name, firstEvent: firstEvent, today: today))
	}

	/// Runs a command that makes one row and returns that row's id.
	private func created(_ kind: EntityKind, by command: Command) async throws -> String {
		let session = session
		let change = try await Core.run { try session.perform(command: command) }
		guard let id = change.entities.first(where: { $0.kind == kind })?.id else {
			throw FfiError.Invalid(message: "The core saved it but didn't name it. Reopen the budget to see it.")
		}
		return id
	}

	func perform(_ command: Command) async throws {
		let session = session
		_ = try await Core.run { try session.perform(command: command) }
	}

	func migrationSuggestions(today: String) async throws -> [MigrationSuggestion] {
		let session = session
		return try await Core.run { try session.migrationSuggestions(today: today) }
	}

	func applyMigration(_ selection: MigrationSelection, today: String) async throws {
		let session = session
		_ = try await Core.run { try session.applyMigration(selection: selection, today: today) }
	}

	func revertMigration(key: String, groupID: String?, confirmed: Bool, today: String) async throws -> MigrationRevert {
		let session = session
		return try await Core.run { try session.revertMigration(key: key, groupId: groupID, confirmed: confirmed, today: today) }
	}

	func appliedMigrations() async throws -> [AppliedMigration] {
		let session = session
		return try await Core.run { try session.appliedMigrations() }
	}

	func migrationOffered() async throws -> Bool {
		let session = session
		return try await Core.run { try session.migrationOffered() }
	}

	func markMigrationOffered() async throws {
		let session = session
		_ = try await Core.run { try session.markMigrationOffered() }
	}

	func setMigrationDismissed(_ key: String, dismissed: Bool) async throws {
		let session = session
		_ = try await Core.run { try session.setMigrationDismissed(key: key, dismissed: dismissed) }
	}

	func setRetention(_ retention: Retention) async throws {
		let session = session
		try await Core.run { try session.setRetention(retention: retention) }
	}
}

/// The core calls this on whichever thread made the call that changed something.
private final class ChangeRelay: ChangeListener, @unchecked Sendable {
	private let lock = NSLock()
	private var handler: (@Sendable () -> Void)?

	func set(_ handler: @escaping @Sendable () -> Void) {
		lock.withLock { self.handler = handler }
	}

	func changed(change: Change) {
		lock.withLock { handler }?()
	}
}
