import Foundation
import VeritasKit

/// Where a window gets its budgets from, and whether file panels are skipped.
/// Only debug builds read the UI-test launch arguments.
enum LaunchEnvironment {
	static func library() -> any BudgetLibrary {
		#if DEBUG
		if let stub = StubLaunch.current {
			return stub.library
		}
		#endif
		return CoreBudgetLibrary()
	}

	static func localSettings() -> any LocalBudgetSettings {
		#if DEBUG
		if StubLaunch.current != nil {
			return InMemoryBudgetSettings()
		}
		#endif
		return BookmarkedBudgetSettings()
	}

	/// UI tests can't drive a folder panel, so the stub picks its folder itself.
	static var backupFolderWithoutPanel: URL? {
		#if DEBUG
		return StubLaunch.current == nil ? nil : StubLaunch.backupFolder
		#else
		return nil
		#endif
	}

	static var budgetToOpenOnLaunch: URL? {
		#if DEBUG
		return StubLaunch.current?.opensBudgetOnLaunch == true ? StubLaunch.budgetDirectory : nil
		#else
		return nil
		#endif
	}

	static var importWithoutPanel: URL? {
		#if DEBUG
		return StubLaunch.current == nil ? nil : StubLaunch.zip
		#else
		return nil
		#endif
	}
}
