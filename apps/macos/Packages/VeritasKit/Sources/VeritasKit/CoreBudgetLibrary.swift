import Foundation
import VeritasCore

/// The library backed by the Rust core. FFI contract v0 has no `Session` yet,
/// so until WS-SESSION exports it every call fails with an `Internal` error.
public struct CoreBudgetLibrary: BudgetLibrary {
	public let budgetsDirectory: URL

	public init(budgetsDirectory: URL = URL.applicationSupportDirectory.appending(path: "Veritas/budgets", directoryHint: .isDirectory)) {
		self.budgetsDirectory = budgetsDirectory
	}

	public func open(directory: URL) async throws -> any BudgetSession {
		throw Self.sessionMissing
	}

	public func importActualZip(_ zip: URL) async throws -> any BudgetSession {
		throw Self.sessionMissing
	}

	private static var sessionMissing: FfiError {
		let version = (try? coreVersion()) ?? "unknown"
		return .Internal(message: "core \(version) doesn't export Session yet (FFI contract v0)")
	}
}
