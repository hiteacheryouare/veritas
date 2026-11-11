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
