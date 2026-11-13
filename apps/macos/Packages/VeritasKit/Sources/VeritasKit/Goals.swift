import Foundation
import VeritasCore

/// One goal as the Budget overview and the Goals screen show it (R-GOAL-1/3,
/// D66). The core decides the status and the monthly amount a goal needs.
public struct GoalSummary: Hashable, Identifiable, Sendable {
	public enum Status: Hashable, Sendable {
		/// On pace to reach the target by `targetMonth`.
		case onTrack
		/// Needs `monthly` a month from now on to reach the target in time.
		case behind(monthly: Int64)
		case reached
		/// Reached, then spent: no refill suggestion until it's reopened (D66).
		case spent
	}

	public let id: String
	public let categoryID: String
	public let name: String
	public let target: Int64
	public let available: Int64
	public let targetMonth: Month?
	public let status: Status

	public init(id: String, categoryID: String, name: String, target: Int64, available: Int64, targetMonth: Month?, status: Status) {
		self.id = id
		self.categoryID = categoryID
		self.name = name
		self.target = target
		self.available = available
		self.targetMonth = targetMonth
		self.status = status
	}

	/// How full the ring is: available over target, 0 to 1.
	public var progress: Double {
		guard target > 0 else { return 0 }
		return min(max(Double(available) / Double(target), 0), 1)
	}
}
