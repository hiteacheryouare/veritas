import Foundation
import VeritasCore

/// Canned upgrade suggestions for UI tests: a pre-ticked goal and a debt
/// kind that needs the user's judgment.
final class StubUpgrade: @unchecked Sendable {
	private let lock = NSLock()
	private var applied: [String] = []
	private var dismissed: Set<String> = []
	private var offered = false

	private static let all: [MigrationSuggestion] = [
		MigrationSuggestion(
			key: "goal:vacation",
			conversion: .goal,
			subjectId: "vacation",
			subjectName: "Vacation",
			confidence: .preTicked,
			signals: [MigrationSignal(code: "s1_template", text: "A target in the category's note, by 2027-06-30", figures: [MigrationFigure(name: "target", value: 300_000)])],
			proposal: MigrationProposal(target: 300_000, targetDate: "2027-06-30", kind: nil, displayName: nil, icon: nil, nativeType: nil, nativeMeta: nil),
			conflicts: [],
			blocked: nil,
			blockedBy: nil,
			confirmLines: ["\"Vacation\" moves to the top of the Goals group. Its budget amounts, notes and transactions stay with it, and every total stays the same."],
			effects: [],
			kindChange: nil,
			goalPreview: MigrationGoalPreview(reachedMonth: nil, monthly: 22_500, monthsLeft: 8, spendingRecorded: false),
			targetInActual: false,
			asksToShowGoalsGroup: false,
			dismissed: false
		),
		MigrationSuggestion(
			key: "kind:carloan",
			conversion: .kind,
			subjectId: "carloan",
			subjectName: "Car Loan",
			confidence: .shown,
			signals: [MigrationSignal(code: "transfers", text: "2 transfers to Car Loan (a debt) in 2 of the last 12 months, 100% of this category's rows", figures: [])],
			proposal: MigrationProposal(target: nil, targetDate: nil, kind: .debtPrincipal, displayName: nil, icon: nil, nativeType: nil, nativeMeta: nil),
			conflicts: [],
			blocked: nil,
			blockedBy: nil,
			confirmLines: [],
			effects: [MigrationEffect(report: "Expenses", month: Month(year: 2026, month: 10), before: 158_550, after: 120_000)],
			kindChange: MigrationKindChange(rows: 2, transfers: 2, amount: 77_100, purchases: 0, kind: .debtPrincipal),
			goalPreview: nil,
			targetInActual: false,
			asksToShowGoalsGroup: false,
			dismissed: false
		)
	]

	func suggestions() -> [MigrationSuggestion] {
		lock.withLock {
			Self.all
				.filter { !applied.contains($0.key) }
				.map { suggestion in
					var copy = suggestion
					copy.dismissed = dismissed.contains(suggestion.key)
					return copy
				}
		}
	}

	func preview(_ selection: MigrationSelection) -> [MigrationSuggestion] {
		let keys = selection.items.map(\.key)
		return suggestions().filter { keys.contains($0.key) }
	}

	func apply(_ selection: MigrationSelection) {
		lock.withLock { applied += selection.items.map(\.key) }
	}

	func appliedItems() -> [AppliedMigration] {
		lock.withLock {
			applied.compactMap { key in
				Self.all.first { $0.key == key }.map { AppliedMigration(key: key, conversion: $0.conversion, subjectId: $0.subjectId, closed: false) }
			}
		}
	}

	func revert(_ key: String, confirmed: Bool) -> MigrationRevert {
		if key.hasPrefix("kind:"), !confirmed {
			return .confirmKind(change: MigrationKindChange(rows: 2, transfers: 2, amount: 77_100, purchases: 0, kind: .debtPrincipal), effects: [], replaces: nil)
		}
		lock.withLock { applied.removeAll { $0 == key } }
		return .reverted(change: Change(entities: [], everything: true), notes: [])
	}

	func wasOffered() -> Bool { lock.withLock { offered } }
	func markOffered() { lock.withLock { offered = true } }

	func setDismissed(_ key: String, _ isDismissed: Bool) {
		lock.withLock {
			if isDismissed { dismissed.insert(key) } else { dismissed.remove(key) }
		}
	}
}
