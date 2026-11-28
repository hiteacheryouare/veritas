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
		if RealCoreLaunch.isOn {
			return CoreBudgetLibrary(budgetsDirectory: RealCoreLaunch.budgetsDirectory)
		}
		#endif
		return CoreBudgetLibrary()
	}

	static func localSettings() -> any LocalBudgetSettings {
		#if DEBUG
		if StubLaunch.current != nil || RealCoreLaunch.isOn {
			return InMemoryBudgetSettings()
		}
		#endif
		return BookmarkedBudgetSettings()
	}

	/// Where the tour remembers it was skipped or finished. UI tests get a
	/// fresh store with the tour already seen, unless they pass `-VeritasTour YES`.
	@MainActor static let tourDefaults: UserDefaults = {
		#if DEBUG
		if StubLaunch.current != nil || RealCoreLaunch.isOn, let defaults = UserDefaults(suiteName: "veritas-ui-test-tour") {
			defaults.removePersistentDomain(forName: "veritas-ui-test-tour")
			defaults.set(!UserDefaults.standard.bool(forKey: "VeritasTour"), forKey: GuidedTour.finishedKey)
			return defaults
		}
		#endif
		return .standard
	}()

	/// The budgets every window's menus offer. UI tests start with none.
	@MainActor static let recentBudgets: RecentBudgets = {
		#if DEBUG
		if StubLaunch.current != nil || RealCoreLaunch.isOn, let defaults = UserDefaults(suiteName: "veritas-ui-test-recents") {
			defaults.removePersistentDomain(forName: "veritas-ui-test-recents")
			return RecentBudgets(defaults: defaults)
		}
		#endif
		return RecentBudgets(defaults: .standard)
	}()

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
