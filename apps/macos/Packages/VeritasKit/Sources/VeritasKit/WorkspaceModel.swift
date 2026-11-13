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
	/// Goes up after every change the core announces, so open screens refetch.
	public private(set) var changeCount = 0
	public var destination = Destination.start
	public var sheet: WorkspaceSheet?
	public var alert: ErrorAlert?
	public var isImportPanelShown = false
	public var newBudgetSetup: NewBudgetSetup?
	public var isPaletteShown = false
	public var isFindShown = false
	public var isDense = false
	public var showsHiddenEnvelopes = false
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
	@ObservationIgnored private let maintenanceInterval: Duration
	@ObservationIgnored private let checkingInterval: Duration

	/// The core asks to be maintained every few minutes, and more often while
	/// its open check runs, because that check can turn the budget read-only.
	public init(
		library: any BudgetLibrary,
		localSettings: any LocalBudgetSettings = InMemoryBudgetSettings(),
		format: MoneyFormat = MoneyFormat(locale: .current),
		maintenanceInterval: Duration = .seconds(300),
		checkingInterval: Duration = .seconds(2)
	) {
		self.library = library
		self.localSettings = localSettings
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
		guard let session else { return [] }
		do {
			return try await session.goals(today: Self.today())
		} catch {
			present(error, isRefresh: true)
			return []
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

	public var canEditDashboards: Bool { session?.canEditDashboards ?? false }

	/// D62: a code Actual doesn't know uses 2 places.
	private func decimalPlaces(for code: String?) async -> UInt8 {
		guard let code, let currencies = try? await library.currencies() else { return 2 }
		return currencies.first { $0.code == code }?.decimalPlaces ?? 2
	}

	/// The budget currency's decimal places (D63): budget cells and goal
	/// amounts use them; the register always uses two.
	public private(set) var budgetDecimalPlaces: UInt8 = 2

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

	/// The local civil date (PLAN §9 #14): time zones never shift it.
	public static func today(_ now: Date = .now, calendar: Calendar = .current) -> String {
		let parts = calendar.dateComponents([.year, .month, .day], from: now)
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

	public func closeBudget() async {
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

	public func go(to destination: Destination) {
		self.destination = destination
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
			let settings = BudgetSettingsModel(session: opened, local: localSettings, budget: overview.directory) { [weak self] error in
				self?.present(error)
			}
			self.settings = settings
			startMaintenance(of: opened)
			budgetDecimalPlaces = await decimalPlaces(for: overview.currencyCode)
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
					await self.refresh()
					await self.settings?.backUpIfDue()
				} catch {
					self.present(error)
				}
			}
		}
	}

	private func apply(_ overview: BudgetOverview) {
		self.overview = overview
		readOnly = overview.readOnly
		if case .account(let id) = destination, !overview.accounts.contains(where: { $0.id == id }) {
			destination = .screen(.accounts)
		}
	}
}
