import Foundation
import VeritasCore

// TEMPORARY, until contract v11. Each property below stands in for a field
// the core doesn't expose yet (requested on PR #68). Each is named exactly
// like the field it waits for, so when `cargo xtask bindings` brings that
// field in, this extension stops compiling and the stand-in gets deleted.
// Both match every associated value by label, so a new one on `.netWorth`
// or `.spending` breaks them too.

public extension WidgetView {
	/// Requested for v11: the net worth card's change over its range, Actual's
	/// `totalChange` (last month less first). Nil with fewer than two months.
	var change: Int64? {
		guard case .netWorth(points: let points) = self, points.count > 1, let first = points.first, let last = points.last else { return nil }
		return last.netWorth - first.netWorth
	}

	/// Requested for v11: the spending card's difference, the compare month
	/// less this month, as Actual's card shows it.
	var difference: Int64? {
		guard case .spending(month: _, spent: let spent, compareMonth: _, compareSpent: let compareSpent) = self else { return nil }
		return compareSpent - spent
	}
}
