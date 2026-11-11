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
	public var destination = Destination.start
	public var sheet: WorkspaceSheet?
	public var alert: ErrorAlert?
	public var isImportPanelShown = false
	public var newBudgetSetup: NewBudgetSetup?
	public var isPaletteShown = false
	public var isFindShown = false
	public var isDense = false
	public let format: MoneyFormat

	@ObservationIgnored private let library: any BudgetLibrary
	@ObservationIgnored private var session: (any BudgetSession)?
	@ObservationIgnored private var retry: (@MainActor () async -> Void)?
	@ObservationIgnored private var maintenance: Task<Void, Never>?
	@ObservationIgnored private let maintenanceInterval: Duration
	@ObservationIgnored private let checkingInterval: Duration

	/// The core asks to be maintained every few minutes, and more often while
	/// its open check runs, because that check can turn the budget read-only.
	public init(
		library: any BudgetLibrary,
		format: MoneyFormat = MoneyFormat(locale: .current),
		maintenanceInterval: Duration = .seconds(300),
		checkingInterval: Duration = .seconds(2)
	) {
		self.library = library
		self.format = format
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
			try await session.exportActualZip(to: destination)
		} catch {
			present(error, during: .export) { [weak self] in await self?.exportActualZip(to: destination) }
		}
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
		sheet = nil
		destination = .start
		phase = .empty
	}

	public func refresh() async {
		guard let session else { return }
		do {
			apply(try await session.overview())
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
		case .integrityReport:
			sheet = .integrityReport
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
					Task { @MainActor in await self?.refresh() }
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
			startMaintenance(of: opened)
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
