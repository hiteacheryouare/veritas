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
	public var destination = Destination.start
	public var sheet: WorkspaceSheet?
	public var alert: ErrorAlert?
	public var isImportPanelShown = false
	public var isPaletteShown = false
	public var isFindShown = false
	public var isDense = false
	public let format: MoneyFormat

	@ObservationIgnored private let library: any BudgetLibrary
	@ObservationIgnored private var session: (any BudgetSession)?
	@ObservationIgnored private var retry: (@MainActor () async -> Void)?

	public init(library: any BudgetLibrary, format: MoneyFormat = MoneyFormat(locale: .current)) {
		self.library = library
		self.format = format
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
		await open { try await library.open(directory: directory) }
	}

	public func importActualZip(_ zip: URL) async {
		let library = library
		await open { try await library.importActualZip(zip) }
	}

	public func exportActualZip(to destination: URL) async {
		guard let session else { return }
		do {
			try await session.exportActualZip(to: destination)
		} catch {
			present(error) { [weak self] in await self?.exportActualZip(to: destination) }
		}
	}

	public func closeBudget() async {
		await session?.close()
		session = nil
		overview = nil
		readOnly = nil
		sheet = nil
		destination = .start
		phase = .empty
	}

	public func refresh() async {
		guard let session else { return }
		do {
			apply(try await session.overview())
		} catch {
			present(error)
		}
	}

	public func present(_ error: any Error, retry: (@MainActor () async -> Void)? = nil) {
		switch ErrorPresentation(error).withoutField {
		case .nothing, .inline:
			break
		case .refresh:
			Task { await refresh() }
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

	private func open(_ opening: @escaping @MainActor () async throws -> any BudgetSession) async {
		let previousPhase = phase
		phase = .working
		do {
			let opened = try await opening()
			let overview = try await opened.overview()
			if let session, session !== opened {
				await session.close()
			}
			session = opened
			apply(overview)
			phase = .open
		} catch {
			phase = previousPhase == .open ? .open : .empty
			present(error) { [weak self] in await self?.open(opening) }
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
