import Foundation
import VeritasCore

/// Budgets backed by the Rust core. Each budget is a folder under `budgetsDirectory`.
public struct CoreBudgetLibrary: BudgetLibrary {
	public let budgetsDirectory: URL

	public init(budgetsDirectory: URL = URL.applicationSupportDirectory.appending(path: "Veritas/budgets", directoryHint: .isDirectory)) {
		self.budgetsDirectory = budgetsDirectory
	}

	public func newBudgetDirectory() -> URL {
		budgetsDirectory.appending(path: UUID().uuidString, directoryHint: .isDirectory)
	}

	public func listBudgets() async throws -> [BudgetListingItem] {
		let path = budgetsDirectory.path(percentEncoded: false)
		guard FileManager.default.fileExists(atPath: path) else { return [] }
		return try await Core.run { try VeritasCore.listBudgets(budgetsDir: path) }.map { listing in
			BudgetListingItem(
				id: listing.id,
				name: listing.name,
				folder: URL(filePath: listing.folder, directoryHint: .isDirectory),
				lastOpened: CoreTime.date(listing.lastOpened)
			)
		}
	}

	public func open(directory: URL) async throws -> any BudgetSession {
		let path = directory.path(percentEncoded: false)
		return CoreBudgetSession(try await Core.cancellable { try await Session.open(directory: path, cancel: $0) })
	}

	public func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession {
		try makeBudgetsDirectory()
		let zipPath = zip.path(percentEncoded: false)
		let path = directory.path(percentEncoded: false)
		return CoreBudgetSession(try await Core.cancellable {
			try await Session.importActualZip(zip: zipPath, intoDirectory: path, cancel: $0)
		})
	}

	public func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession {
		try makeBudgetsDirectory()
		let path = directory.path(percentEncoded: false)
		let format = NumberFormat(currency: budget.currencyCode.isEmpty ? nil : budget.currencyCode, style: budget.numberStyle, hideDecimals: false)
		let options = NewBudgetOptions(
			retirement401k: budget.hasRetirementPlan,
			hsa: budget.hasHealthSavingsAccount,
			accounts: budget.accounts.map { NewAccount(name: $0.name, kind: $0.type, startingBalance: $0.balance) }
		)
		return CoreBudgetSession(try await Core.cancellable {
			try await Session.createBudget(name: budget.name, directory: path, format: format, options: options, cancel: $0)
		})
	}

	/// Actual's own currency list, in its order. "None" has an empty code.
	public func currencies() async throws -> [CurrencyOption] {
		try await Core.run { try VeritasCore.currencies() }.map { currency in
			CurrencyOption(code: currency.code, name: currency.name, symbol: currency.symbol, numberStyle: currency.numberFormat)
		}
	}

	public func evaluateAmount(_ text: String, style: NumberStyle) async throws -> Int64 {
		try await Core.run { try VeritasCore.evaluateAmount(text: text, style: style) }
	}

	private func makeBudgetsDirectory() throws {
		do {
			try FileManager.default.createDirectory(at: budgetsDirectory, withIntermediateDirectories: true)
		} catch {
			throw FfiError.Io(message: "Can't create \(budgetsDirectory.path(percentEncoded: false)). Check the disk has space.")
		}
	}
}

enum Core {
	/// Synchronous core calls block while SQLite works, so they run off the main actor.
	static func run<T: Sendable>(_ call: @escaping @Sendable () throws -> T) async throws -> T {
		try await self.call { try await Task.detached(priority: .userInitiated, operation: call).value }
	}

	/// Long core work gets its own token; cancelling the Swift task cancels it,
	/// and the core then answers `Cancelled`.
	static func cancellable<T>(_ work: (CancelToken) async throws -> T) async throws -> T {
		let token = CancelToken()
		return try await withTaskCancellationHandler {
			try await call { try await work(token) }
		} onCancel: {
			token.cancel()
		}
	}

	/// Everything the core throws is an `FfiError`. A cancelled Swift task becomes `Cancelled`.
	static func call<T>(_ call: () async throws -> T) async throws -> T {
		do {
			return try await call()
		} catch let error as FfiError {
			throw error
		} catch is CancellationError {
			throw FfiError.Cancelled
		} catch {
			throw FfiError.Internal(message: String(describing: error))
		}
	}

}
