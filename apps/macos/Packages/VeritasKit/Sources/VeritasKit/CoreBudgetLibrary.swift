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

	public func open(directory: URL) async throws -> any BudgetSession {
		let path = directory.path(percentEncoded: false)
		return CoreBudgetSession(try await Core.run { try Session.open(directory: path) })
	}

	public func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession {
		let budgets = budgetsDirectory
		let zipPath = zip.path(percentEncoded: false)
		let directoryPath = directory.path(percentEncoded: false)
		let session = try await Core.run {
			try FileManager.default.createDirectory(at: budgets, withIntermediateDirectories: true)
			return try Session.importActualZip(zip: zipPath, intoDirectory: directoryPath)
		}
		return CoreBudgetSession(session)
	}

	public func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession {
		throw Core.missing("Session.create_budget")
	}

	public func currencies() async throws -> [CurrencyOption] {
		throw Core.missing("currencies")
	}

	public func evaluateAmount(_ text: String) async throws -> Int64 {
		throw Core.missing("evaluate_amount")
	}
}

enum Core {
	/// Core calls block while SQLite works, so they run off the main actor.
	static func run<T: Sendable>(_ call: @escaping @Sendable () throws -> T) async throws -> T {
		do {
			return try await Task.detached(priority: .userInitiated, operation: call).value
		} catch let error as FfiError {
			throw error
		} catch {
			throw FfiError.Io(message: error.localizedDescription)
		}
	}

	static func missing(_ export: String) -> FfiError {
		.Internal(message: "the core doesn't export \(export) yet")
	}
}
