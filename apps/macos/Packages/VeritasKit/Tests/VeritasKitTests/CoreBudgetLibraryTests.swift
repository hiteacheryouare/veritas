import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// Runs the real core against fixtures/synthetic/small.zip.
struct CoreBudgetLibraryTests {
	let budgets: URL
	let library: CoreBudgetLibrary

	init() {
		budgets = URL.temporaryDirectory.appending(path: "veritas-kit-\(UUID().uuidString)", directoryHint: .isDirectory)
		library = CoreBudgetLibrary(budgetsDirectory: budgets)
	}

	private var smallZip: URL {
		URL(filePath: #filePath)
			.deletingLastPathComponent()
			.appending(path: "../../../../../../fixtures/synthetic/small.zip")
			.standardizedFileURL
	}

	@Test func importingAnActualZipOpensAWritableBudgetInTheBudgetsFolder() async throws {
		defer { try? FileManager.default.removeItem(at: budgets) }
		let session = try await library.importActualZip(smallZip, into: library.newBudgetDirectory())
		let overview = try await session.overview()
		#expect(overview.readOnly == nil)
		#expect(!overview.canUndo && !overview.canRedo)
		#expect(overview.directory.path(percentEncoded: false).hasPrefix(budgets.path(percentEncoded: false)))
		await session.close()
	}

	@Test func aSecondWindowOnTheSameBudgetIsReadOnly() async throws {
		// R-REL-6
		defer { try? FileManager.default.removeItem(at: budgets) }
		let first = try await library.importActualZip(smallZip, into: library.newBudgetDirectory())
		let directory = try await first.overview().directory
		let second = try await library.open(directory: directory)
		#expect(try await second.overview().readOnly == .openElsewhere)
		await second.close()
		await first.close()
		let reopened = try await library.open(directory: directory)
		#expect(try await reopened.overview().readOnly == nil)
		await reopened.close()
	}

	@Test func aZipThatIsntABudgetFailsWithTheCoresError() async throws {
		defer { try? FileManager.default.removeItem(at: budgets) }
		try FileManager.default.createDirectory(at: budgets, withIntermediateDirectories: true)
		let notABudget = budgets.appending(path: "notes.zip")
		try Data("not a zip".utf8).write(to: notABudget)
		await #expect {
			_ = try await library.importActualZip(notABudget, into: library.newBudgetDirectory())
		} throws: { error in
			guard let error = error as? FfiError else { return false }
			switch error {
			case .NotAnActualBudget, .Io: return true
			default: return false
			}
		}
	}

	@Test func exportIsDisabledUntilTheCoreExportsIt() async throws {
		defer { try? FileManager.default.removeItem(at: budgets) }
		let session = try await library.importActualZip(smallZip, into: library.newBudgetDirectory())
		#expect(!session.canExport)
		await session.close()
	}
}
