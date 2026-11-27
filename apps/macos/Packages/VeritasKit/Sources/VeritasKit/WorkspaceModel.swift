import Foundation
import Observation
import VeritasCore

/// One window: the budget it has open, where it is, and what it's showing on top.
@MainActor
@Observable
public final class WorkspaceModel {
	public enum Phase: Hashable, Sendable {
		case empty
		case working
		case open
	}

	public private(set) var phase = Phase.empty
	public private(set) var overview: BudgetOverview?
	public private(set) var readOnly: ReadOnlyKind?
	public private(set) var canExport = false
	public private(set) var settings: BudgetSettingsModel?
	public private(set) var integrityReport: IntegrityReport?
	public private(set) var budgetList: [BudgetListingItem]?
	public var isOpenPickerShown = false
	/// Shared by every window, so each one's menus list the same budgets.
	public let recents: RecentBudgets
	/// Goes up after every change the core announces, so open screens refetch.
	public private(set) var changeCount = 0
	public var destination = Destination.start
	public var sheet: WorkspaceSheet?
	public var alert: ErrorAlert?
	public var isImportPanelShown = false
	public var newBudgetSetup: NewBudgetSetup?
	public var isPaletteShown = false
	public var isFindShown = false
	public var isAssistantShown = false
	public var isDense = false
	public var showsHiddenEnvelopes = false
	/// The what-ifs Forecast compares against the baseline, in the order
	/// chosen, kept here so the What-ifs page and Forecast share them.
	public var comparedScenarios: [String] = []
	/// The budget's own format once the core reports it, else the Mac's.
	public var format: MoneyFormat {
		overview?.format ?? fallbackFormat
	}

	@ObservationIgnored private let library: any BudgetLibrary
	@ObservationIgnored private let localSettings: any LocalBudgetSettings
	@ObservationIgnored private let fallbackFormat: MoneyFormat
	@ObservationIgnored private var session: (any BudgetSession)?
	@ObservationIgnored private var retry: (@MainActor () async -> Void)?
	@ObservationIgnored private var maintenance: Task<Void, Never>?
	/// The folder this window holds in `recents`, while it opens or has it open.
	@ObservationIgnored private var held: URL?
	@ObservationIgnored private let maintenanceInterval: Duration
	@ObservationIgnored private let checkingInterval: Duration

	/// The core asks to be maintained every few minutes, and more often while
	/// its open check runs, because that check can turn the budget read-only.
	public init(
		library: any BudgetLibrary,
		localSettings: any LocalBudgetSettings = InMemoryBudgetSettings(),
		recents: RecentBudgets? = nil,
		format: MoneyFormat = MoneyFormat(locale: .current),
		maintenanceInterval: Duration = .seconds(300),
		checkingInterval: Duration = .seconds(2)
	) {
		self.library = library
		self.localSettings = localSettings
		self.recents = recents ?? RecentBudgets(defaults: nil, locate: { _ in .here })
		fallbackFormat = format
		self.maintenanceInterval = maintenanceInterval
		self.checkingInterval = checkingInterval
	}

	public var canUndo: Bool {
		phase == .open && overview?.canUndo == true
	}

	public var canRedo: Bool {
		phase == .open && overview?.canRedo == true
	}

	public var paletteIndex: PaletteIndex {
		PaletteIndex(accounts: overview?.accounts ?? [], format: format)
	}

	public var canToggleDense: Bool {
		phase == .open && destination == .screen(.budget)
	}

	public func title(of destination: Destination) -> String {
		switch destination {
		case .screen(let screen): screen.title
		case .account(let id): overview?.accounts.first { $0.id == id }?.name ?? Screen.accounts.title
		}
	}

	public func reopen(directory: URL) async {
		let library = library
		await open(during: .open) { try await library.open(directory: directory) }
	}

	/// A new window with nothing to restore opens the budget used last, so
	/// closing the window and coming back from the Dock or a relaunch finds it.
	public func reopenMostRecent() async {
		guard phase == .empty, let budget = recents.budgetToReopen else { return }
		hold(budget.folder)
		await reopen(directory: budget.folder)
		if phase != .open { hold(nil) }
	}

	/// Swaps the folder this window holds in `recents` for `folder`.
	private func hold(_ folder: URL?) {
		if let held, let folder, RecentBudgets.key(held) == RecentBudgets.key(folder) { return }
		if let held { recents.release(held) }
		held = folder
		if let folder { recents.hold(folder) }
	}

	/// Holds the zip's sandbox access for each attempt, so a retry can still read it.
	public func importActualZip(_ zip: URL) async {
		let library = library
		let directory = library.newBudgetDirectory()
		await open(during: .importing) {
			let access = zip.startAccessingSecurityScopedResource()
			defer { if access { zip.stopAccessingSecurityScopedResource() } }
			return try await library.importActualZip(zip, into: directory)
		}
	}

	public func startNewBudget() {
		newBudgetSetup = NewBudgetSetup(library: library)
	}

	public func finishNewBudget(_ budget: NewBudget) async {
		newBudgetSetup = nil
		await createBudget(budget)
	}

	/// The same folder is reused if the user retries, so a retry never makes a second budget.
	public func createBudget(_ budget: NewBudget) async {
		let library = library
		let directory = library.newBudgetDirectory()
		await open(during: .creating) { try await library.createBudget(budget, into: directory) }
	}

	public func exportActualZip(to destination: URL) async {
		guard let session, canExport else { return }
		do {
			let findings = try await session.exportActualZip(to: destination)
			if !findings.isEmpty {
				integrityReport = .export(findings: findings)
				sheet = .integrityReport
			}
		} catch {
			present(error, during: .export) { [weak self] in await self?.exportActualZip(to: destination) }
		}
	}

	/// The open budget's own check. Findings arrive when the background check finishes.
	public func showIntegrityReport() {
		integrityReport = .budget(findings: nil)
		sheet = .integrityReport
		guard let session else { return }
		Task {
			do {
				integrityReport = .budget(findings: try await session.integrityFindings())
			} catch {
				present(error)
			}
		}
	}

	public var numberStyle: NumberStyle {
		overview?.numberStyle ?? .commaDot
	}

	public func budgetMonth(_ month: Month) async -> BudgetMonth? {
		guard let session else { return nil }
		do {
			return try await session.budgetMonth(month, today: Self.today())
		} catch {
			present(error, isRefresh: true)
			return nil
		}
	}

	public func goals() async -> [GoalSummary] {
		await goalList().goals
	}

	/// The goals with the core's 0-place note, for the Goals screen.
	public func goalList() async -> GoalList {
		let none = GoalList(goals: [], mixedScaleNote: nil, zeroPlaces: false)
		guard let session else { return none }
		do {
			return try await session.goals(today: Self.today())
		} catch {
			present(error, isRefresh: true)
			return none
		}
	}

	public func goalClosePreview(_ goalID: String, releaseTo: String?) async -> GoalClosePreview? {
		guard let session else { return nil }
		do {
			return try await session.goalClosePreview(goalID, releaseTo: releaseTo, today: Self.today())
		} catch {
			present(error)
			return nil
		}
	}

	public func categoryHistory(_ categoryID: String) async -> CategoryHistory? {
		guard let session else { return nil }
		do {
			return try await session.categoryHistory(categoryID)
		} catch {
			present(error)
			return nil
		}
	}

	/// A report read against the open budget. A failure is shown here and
	/// comes back as `nil`; a cancelled read, as a screen moves on, is quiet.
	public func report<T: Sendable>(_ read: @Sendable (any BudgetSession) async throws -> T) async -> T? {
		guard let session else { return nil }
		do {
			return try await read(session)
		} catch FfiError.Cancelled {
			return nil
		} catch {
			present(error, isRefresh: true)
			return nil
		}
	}

	/// A read whose failure the caller shows itself, next to the field that
	/// caused it, so no alert repeats it.
	public func quietly<T: Sendable>(_ read: @Sendable (any BudgetSession) async throws -> T) async throws -> T? {
		guard let session else { return nil }
		return try await read(session)
	}

	/// A read whose error the caller shows where it happened, such as the
	/// report editor's preview. Nil when no budget is open or it was cancelled.
	public func attempt<T: Sendable>(_ read: @Sendable (any BudgetSession) async throws -> T) async -> Result<T, any Error>? {
		guard let session else { return nil }
		do {
			return .success(try await read(session))
		} catch FfiError.Cancelled {
			return nil
		} catch {
			return .failure(error)
		}
	}

	/// A write that isn't a `Command`, such as a dashboard edit. Like
	/// `perform`, it shows its own error and refreshes on success.
	@discardableResult
	public func change<T: Sendable>(_ write: @Sendable (any BudgetSession) async throws -> T) async -> T? {
		guard let session else { return nil }
		do {
			let result = try await write(session)
			changeCount += 1
			await refresh()
			return result
		} catch {
			present(error)
			return nil
		}
	}

	/// A write that isn't a `Command`, from a sheet that shows the core's
	/// `Invalid` message beside its own fields, as `perform(_:inline:)` does.
	@discardableResult
	public func change<T: Sendable>(_ write: @Sendable (any BudgetSession) async throws -> T, inline: (String) -> Void) async -> T? {
		guard let session else { return nil }
		do {
			let result = try await write(session)
			changeCount += 1
			await refresh()
			return result
		} catch {
			if case .inline(let message) = ErrorPresentation(error) {
				inline(message)
			} else {
				present(error)
			}
			return nil
		}
	}

	public var canEditDashboards: Bool { session?.canEditDashboards ?? false }
	public var canPlanTransfers: Bool { session?.canPlanTransfers ?? false }

	/// Where ⌘N adds a transaction: the account on screen, else the first in the sidebar.
	public var currentAccountID: String {
		if case .account(let id) = destination { return id }
		return overview?.accounts.first { !$0.isInactive }?.id ?? ""
	}

	/// The open budget's reads for the assistant, with no way to write.
	public var readOnlyQueries: (any ReadOnlyBudgetQueries)? {
		session.map(BudgetSessionQueries.init)
	}

	/// The budget currency's decimal places (D63): budget cells and goal
	/// amounts use them; the register always uses two. They arrive with the
	/// overview, so they are never another budget's.
	public var budgetDecimalPlaces: UInt8 {
		format.decimalPlaces
	}

	/// What every budget-cell and goal amount field asks the core, at the
	/// open budget's places. Fields build it when they open, never earlier.
	public var budgetAmountReader: any AmountReader {
		CoreAmountReader(budgetDecimalPlaces: budgetDecimalPlaces, hideDecimals: format.hidesDecimals)
	}

	/// Runs one command. Errors are shown here; `false` means nothing changed.
	@discardableResult
	public func perform(_ command: Command) async -> Bool {
		guard let session else { return false }
		do {
			try await session.perform(command)
			changeCount += 1
			await refresh()
			return true
		} catch {
			present(error) { [weak self] in await self?.perform(command) }
			return false
		}
	}

	/// A command from a sheet that shows the core's `Invalid` message beside
	/// its own fields. Every other error is shown as `perform` shows it.
	@discardableResult
	public func perform(_ command: Command, inline: (String) -> Void) async -> Bool {
		guard let session else { return false }
		do {
			try await session.perform(command)
			changeCount += 1
			await refresh()
			return true
		} catch {
			if case .inline(let message) = ErrorPresentation(error) {
				inline(message)
			} else {
				present(error)
			}
			return false
		}
	}

	/// The local civil date (PLAN §9 #14): time zones never shift it, and it
	/// is Gregorian whatever calendar `calendar` reads dates in.
	public nonisolated static func today(_ now: Date = .now, calendar: Calendar = .current) -> String {
		let parts = calendar.civil.dateComponents([.year, .month, .day], from: now)
		return String(format: "%04d-%02d-%02d", parts.year ?? 0, parts.month ?? 1, parts.day ?? 1)
	}

	public func showOpenPicker() {
		budgetList = nil
		isOpenPickerShown = true
		Task {
			do {
				budgetList = try await library.listBudgets()
			} catch {
				isOpenPickerShown = false
				present(error, during: .open)
			}
		}
	}

	public func openListedBudget(_ budget: BudgetListingItem) async {
		isOpenPickerShown = false
		await reopen(directory: budget.folder)
	}

	/// Closes this window's budget as Close Budget does, so its lock and
	/// snapshots settle, then opens the chosen one.
	public func switchTo(_ budget: RecentBudgets.Entry) async {
		guard phase != .working else { return }
		if overview.map({ RecentBudgets.key($0.directory) }) == budget.id { return }
		await closeBudget()
		await reopen(directory: budget.folder)
		if phase != .open { recents.forgetMissing() }
	}

	public func undo() async {
		guard let session, canUndo else { return }
		do {
			try await session.undo()
			await refresh()
		} catch {
			present(error)
		}
	}

	public func redo() async {
		guard let session, canRedo else { return }
		do {
			try await session.redo()
			await refresh()
		} catch {
			present(error)
		}
	}

	/// Working until the session has closed, so nothing opens or switches meanwhile.
	public func closeBudget() async {
		phase = .working
		hold(nil)
		maintenance?.cancel()
		maintenance = nil
		await session?.close()
		session = nil
		overview = nil
		readOnly = nil
		canExport = false
		settings = nil
		integrityReport = nil
		sheet = nil
		comparedScenarios = []
		destination = .start
		phase = .empty
	}

	public func refresh() async {
		guard let session else { return }
		do {
			apply(try await session.overview())
			if sheet == .integrityReport, integrityReport == .budget(findings: nil) {
				integrityReport = .budget(findings: try await session.integrityFindings())
			}
		} catch {
			present(error, during: .open, isRefresh: true)
		}
	}

	/// A `NotFound` asks for a refresh, except from a refresh, which would loop.
	public func present(
		_ error: any Error,
		during operation: Operation = .change,
		isRefresh: Bool = false,
		retry: (@MainActor () async -> Void)? = nil
	) {
		switch ErrorPresentation(error, during: operation).withoutField {
		case .nothing, .inline:
			break
		case .refresh:
			if !isRefresh {
				Task { await refresh() }
			}
		case .readOnly(let kind):
			readOnly = kind
		case .integrityReport(let problems):
			if operation == .importing || session == nil {
				integrityReport = .refusedImport(problems: problems)
				sheet = .integrityReport
			} else {
				showIntegrityReport()
			}
		case .alert(let content):
			self.retry = content.offersRetry ? retry : nil
			alert = content
		}
	}

	public func retryFailedAction() async {
		let action = retry
		retry = nil
		await action?()
	}

	/// A bill another screen asked the Bills screen to open, such as a bill's rule on the Rules page.
	public var pendingBillID: String?

	public func go(to destination: Destination) {
		self.destination = destination == .screen(.insights) ? .screen(.forecast) : destination
		isFindShown = false
	}

	public func perform(_ command: PaletteCommand) {
		sheet = command.sheet
	}

	public func activate(_ item: PaletteItem) {
		isPaletteShown = false
		switch item.target {
		case .go(let destination): go(to: destination)
		case .perform(let command): perform(command)
		}
	}

	public func toggleDense() {
		guard canToggleDense else { return }
		isDense.toggle()
	}

	private func open(
		during operation: Operation,
		_ opening: @escaping @MainActor () async throws -> any BudgetSession
	) async {
		let previousPhase = phase
		phase = .working
		do {
			let opened = try await opening()
			let overview: BudgetOverview
			do {
				overview = try await opened.overview()
				try await opened.observeChanges { [weak self] in
					Task { @MainActor in
						self?.changeCount += 1
						await self?.refresh()
					}
				}
			} catch {
				await opened.close()
				throw error
			}
			if let session, session !== opened {
				await session.close()
			}
			session = opened
			canExport = opened.canExport
			apply(overview)
			phase = .open
			hold(overview.directory)
			recents.opened(name: overview.name, folder: overview.directory)
			let settings = BudgetSettingsModel(session: opened, local: localSettings, budget: overview.directory) { [weak self] error in
				self?.present(error)
			}
			self.settings = settings
			do {
				try await opened.maintain()
			} catch {
				present(error)
			}
			// maintain() may have changed state(): a failed schedule run or snapshot shows its banner now.
			await refresh()
			startMaintenance(of: opened)
			await settings.connectBackupFolder()
			await settings.backUpIfDue()
		} catch {
			phase = previousPhase == .open ? .open : .empty
			present(error, during: operation) { [weak self] in await self?.open(during: operation, opening) }
		}
	}

	private func startMaintenance(of session: any BudgetSession) {
		maintenance?.cancel()
		maintenance = Task { [weak self] in
			while !Task.isCancelled {
				let interval = self?.overview?.isChecking == true ? self?.checkingInterval : self?.maintenanceInterval
				guard let interval else { return }
				try? await Task.sleep(for: interval)
				guard !Task.isCancelled, let self, self.session === session else { return }
				do {
					try await session.maintain()
					guard !Task.isCancelled, self.session === session else { return }
					await self.refresh()
					await self.settings?.backUpIfDue()
				} catch {
					// Closing or reopening can cut a call short; that failure belongs to a session that's gone.
					guard !Task.isCancelled, self.session === session else { return }
					self.present(error)
				}
			}
		}
	}

	private func apply(_ overview: BudgetOverview) {
		self.overview = overview
		readOnly = overview.readOnly
		recents.renamed(overview.name, folder: overview.directory)
		if case .account(let id) = destination, !overview.accounts.contains(where: { $0.id == id }) {
			destination = .screen(.accounts)
		}
	}
}
