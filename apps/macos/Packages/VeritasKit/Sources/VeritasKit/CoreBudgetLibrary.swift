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
		return CoreBudgetSession(try await Core.call { try await Session.open(directory: path) })
	}

	public func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession {
		try makeBudgetsDirectory()
		let zipPath = zip.path(percentEncoded: false)
		let path = directory.path(percentEncoded: false)
		return CoreBudgetSession(try await Core.call { try await Session.importActualZip(zip: zipPath, intoDirectory: path) })
	}

	public func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession {
		guard budget.accounts.isEmpty else {
			throw Core.missing("starting accounts in createBudget")
		}
		try makeBudgetsDirectory()
		let path = directory.path(percentEncoded: false)
		let format = NumberFormat(currency: budget.currencyCode, style: budget.numberStyle, hideDecimals: false)
		let options = NewBudgetOptions(retirement401k: budget.hasRetirementPlan, hsa: budget.hasHealthSavingsAccount)
		return CoreBudgetSession(try await Core.call {
			try await Session.createBudget(name: budget.name, directory: path, format: format, options: options)
		})
	}

	/// Until the core lists the currencies Actual supports, setup offers the Mac's own.
	public func currencies() async throws -> [CurrencyOption] {
		let locale = Locale.current
		let code = locale.currency?.identifier ?? "USD"
		let style: NumberStyle = switch (locale.groupingSeparator, locale.decimalSeparator) {
		case (".", ","): .dotComma
		case ("'", _), ("’", _): .apostropheDot
		case (" ", ","), ("\u{202F}", ","), ("\u{00A0}", ","): .spaceComma
		default: .commaDot
		}
		let name = locale.localizedString(forCurrencyCode: code) ?? code
		return [CurrencyOption(code: code, name: name, symbol: MoneyFormat.symbol(forCurrency: code), numberStyle: style)]
	}

	public func evaluateAmount(_ text: String) async throws -> Int64 {
		throw Core.missing("evaluate_amount")
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

	static func missing(_ export: String) -> FfiError {
		.Internal(message: "the core doesn't export \(export) yet")
	}
}
