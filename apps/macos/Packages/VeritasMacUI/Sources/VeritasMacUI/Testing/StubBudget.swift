#if DEBUG
import Foundation
import VeritasCore
import VeritasKit

/// The approved mockup's November 2026, kept in memory so UI tests can budget,
/// move and cover against known numbers. Only debug builds have it.
final class StubBudget: @unchecked Sendable {
	struct Envelope {
		let id: String
		var name: String
		var budgeted: Int64
		let spent: Int64
		let carriedIn: Int64
		var note: String?
		var carryover = false
		var hidden = false
	}

	struct Group {
		let id: String
		var name: String
		let isIncome: Bool
		var envelopes: [Envelope]
		var hidden = false
	}

	static let month = Month(year: 2026, month: 11)

	private let lock = NSLock()
	private var groups: [Group] = [
		Group(id: "everyday", name: "Everyday", isIncome: false, envelopes: [
			Envelope(id: "groceries", name: "Groceries", budgeted: 41_200, spent: -31_862, carriedIn: 0),
			Envelope(id: "dining", name: "Dining out", budgeted: 18_000, spent: -21_475, carriedIn: 0),
			Envelope(id: "gas", name: "Gas", budgeted: 14_000, spent: -9_610, carriedIn: 0),
			Envelope(id: "household", name: "Household", budgeted: 6_000, spent: -5_120, carriedIn: 0)
		]),
		Group(id: "housing", name: "Housing and utilities", isIncome: false, envelopes: [
			Envelope(id: "rent", name: "Rent", budgeted: 165_000, spent: -165_000, carriedIn: 0),
			Envelope(id: "electric", name: "Electric", budgeted: 9_000, spent: 0, carriedIn: 0),
			Envelope(id: "internet", name: "Internet", budgeted: 7_000, spent: -7_000, carriedIn: 0),
			Envelope(id: "landline", name: "Landline", budgeted: 0, spent: 0, carriedIn: 0, hidden: true)
		]),
		Group(id: "personal", name: "Personal", isIncome: false, envelopes: [
			Envelope(id: "hobbies", name: "Hobbies", budgeted: 7_500, spent: -1_299, carriedIn: 0),
			Envelope(id: "gifts", name: "Gifts", budgeted: 4_000, spent: 0, carriedIn: 8_000, note: "Mom's birthday in December")
		]),
		Group(id: "savingsdebt", name: "Savings and debt", isIncome: false, envelopes: [
			Envelope(id: "principal", name: "Car loan payment", budgeted: 0, spent: 0, carriedIn: 0),
			Envelope(id: "interest", name: "Loan interest", budgeted: 0, spent: 0, carriedIn: 0),
			Envelope(id: "savingsfund", name: "Savings", budgeted: 0, spent: 0, carriedIn: 0),
			Envelope(id: "investing", name: "Investments", budgeted: 0, spent: 0, carriedIn: 0)
		]),
		Group(id: "income", name: "Income", isIncome: true, envelopes: [
			Envelope(id: "paycheck", name: "Paycheck", budgeted: 0, spent: 512_640, carriedIn: 0)
		])
	]
	private var held: Int64 = 0
	private var created = 0
	private var goalList: [GoalSummary] = [
		StubBudget.goal("laptop", "New laptop", target: 150_000, available: 112_000, date: "2027-03-01", state: .active, monthly: 7_600, pace: .onTrack(etaMonth: Month(year: 2027, month: 3)), icon: "laptopcomputer"),
		StubBudget.goal("japan", "Japan trip", target: 600_000, available: 231_000, date: "2027-09-01", state: .active, monthly: 33_546, monthsLeft: 11, pace: .behind(neededMonthly: 33_546), icon: "🗾"),
		StubBudget.goal("tires", "Winter tires", target: 85_000, available: 2_000, date: nil, state: .reachedSpent, spent: 83_000),
		StubBudget.goal("vet", "Vet bill", target: 120_418, available: 0, date: nil, state: .closed, spent: 120_418, closed: Month(year: 2026, month: 8))
	]
	/// Envelopes the stub says have history, so delete turns into Archive.
	private let withHistory: Set<String> = ["groceries", "dining", "gas", "household", "rent", "electric", "internet", "hobbies", "gifts"]
	/// Gifts is charitable, so moving its history elsewhere shows TAX-8's warning.
	private let taxBuckets: [String: String] = ["gifts": "charitable"]

	func createEnvelope(in groupID: String, name: String) throws -> String {
		try perform(.createCategory(groupId: groupID, name: name))
		return lock.withLock { "new-\(created)" }
	}

	/// The stub has one set of figures, shown for whichever 2026 month is asked.
	static func clamped(_ month: Month) -> Month {
		guard month.year == 2026 else { return month.year < 2026 ? Month(year: 2026, month: 1) : Month(year: 2026, month: 12) }
		return month
	}

	func month(_ month: Month) -> BudgetMonth {
		lock.withLock {
			let rows = groups.map { group in
				EnvelopeGroup(
					id: group.id, name: group.name, isIncome: group.isIncome, hidden: group.hidden,
					budgeted: group.envelopes.reduce(0) { $0 + $1.budgeted },
					spent: group.envelopes.reduce(0) { $0 + $1.spent },
					available: group.isIncome ? 0 : group.envelopes.reduce(0) { $0 + available($1) },
					envelopes: group.envelopes.map { row($0, isIncome: group.isIncome) }
				)
			}
			let income = groups.filter(\.isIncome).flatMap(\.envelopes).reduce(0) { $0 + $1.spent }
			let budgeted = groups.filter { !$0.isIncome }.flatMap(\.envelopes).reduce(0) { $0 + $1.budgeted }
			return BudgetMonth(
				month: Self.clamped(month), firstMonth: Month(year: 2026, month: 1), lastMonth: Month(year: 2026, month: 12),
				toBudget: income - budgeted - held, toBudgetTone: Self.toBudgetTone(income - budgeted - held),
				income: income, fromLastMonth: 0, overspentLastMonth: 0,
				budgeted: budgeted, forNextMonth: held, spent: 0, available: 0, groups: rows, mixedScaleNote: nil
			)
		}
	}

	/// The To Budget popover (TB-10.1): Assign to underfunded covers the stub's
	/// overspent envelopes; the stub has no stored monthly goals, and Fund goals'
	/// monthly has nothing to fund.
	func toBudgetActions(_ month: Month) -> ToBudgetActions {
		let sheet = self.month(month)
		let fills = lock.withLock { overspentFills(toBudget: sheet.toBudget) }
		let empty: ToBudgetBlock? = sheet.toBudget < 0 ? .overbudgeted(amount: -sheet.toBudget) : sheet.toBudget == 0 ? .zero : nil
		let assign = ToBudgetPlanView(
			block: empty ?? (fills.isEmpty ? .nothingUnderfunded : nil),
			fills: empty == nil ? fills : [],
			envelopes: empty == nil ? UInt32(fills.count) : 0,
			total: empty == nil ? fills.reduce(0) { $0 + $1.amount } : 0,
			hiddenLeftOut: 0,
			skipped: []
		)
		let goals = ToBudgetPlanView(block: empty ?? .noGoalNeedsFunding, fills: [], envelopes: 0, total: 0, hiddenLeftOut: 0, skipped: [])
		return ToBudgetActions(
			month: sheet.month,
			toBudget: sheet.toBudget,
			state: sheet.toBudget < 0 ? .overbudgeted : sheet.toBudget == 0 ? .zero : .positive,
			assign: assign,
			fundGoals: goals,
			hold: ToBudgetHold(offered: true, block: empty, defaultAmount: max(sheet.toBudget, 0), held: sheet.forNextMonth, autoHeld: 0, canReset: sheet.forNextMonth != 0)
		)
	}

	/// Overspent expense envelopes in screen order, each covered from what To
	/// Budget has left. The caller holds the lock.
	private func overspentFills(toBudget: Int64) -> [ToBudgetFill] {
		var left = max(toBudget, 0)
		var fills: [ToBudgetFill] = []
		for envelope in groups.filter({ !$0.isIncome && !$0.hidden }).flatMap(\.envelopes) where !envelope.hidden {
			let need = -available(envelope)
			let amount = min(need, left)
			guard need > 0, amount > 0 else { continue }
			left -= amount
			fills.append(ToBudgetFill(categoryId: envelope.id, name: envelope.name, reason: .overspent, goalId: nil, need: need, amount: amount))
		}
		return fills
	}

	func perform(_ command: Command) throws {
		try lock.withLock {
			switch command {
			case .assignToUnderfunded:
				let income = groups.filter(\.isIncome).flatMap(\.envelopes).reduce(0) { $0 + $1.spent }
				let budgeted = groups.filter { !$0.isIncome }.flatMap(\.envelopes).reduce(0) { $0 + $1.budgeted }
				let fills = overspentFills(toBudget: income - budgeted - held)
				guard !fills.isEmpty else { throw FfiError.Invalid(message: "Nothing is underfunded.") }
				for fill in fills {
					try update(fill.categoryId) { $0.budgeted += fill.amount }
				}
			case .fundGoalsMonthly:
				throw FfiError.Invalid(message: "No active goal needs funding.")
			case .setBudget(_, let id, let amount):
				try update(id) { $0.budgeted = amount }
			case .moveBudget(_, let amount, let from, let to):
				if let from { try update(from) { $0.budgeted -= amount } }
				if let to { try update(to) { $0.budgeted += amount } }
			case .coverOverspending(_, let id, let from, _):
				guard let need = envelope(id).map({ -available($0) }), need > 0 else { return }
				if let from { try update(from) { $0.budgeted -= need } }
				try update(id) { $0.budgeted += need }
			case .holdForNextMonth(_, let amount):
				held += amount
			case .resetHold:
				held = 0
			case .setCarryover(_, let id, let carryover):
				try update(id) { $0.carryover = carryover }
			case .setCategoryNote(let id, let text):
				let note = text.trimmingCharacters(in: .whitespacesAndNewlines)
				try update(id) { $0.note = note.isEmpty ? nil : note }
			case .createCategory(let groupID, let name):
				created += 1
				guard let g = groups.firstIndex(where: { $0.id == groupID }) else { throw FfiError.NotFound(entity: "group", id: groupID) }
				groups[g].envelopes.insert(Envelope(id: "new-\(created)", name: name, budgeted: 0, spent: 0, carriedIn: 0), at: 0)
			case .renameCategory(let id, let name):
				try update(id) { $0.name = name }
			case .setCategoryHidden(let id, let hidden):
				try update(id) { $0.hidden = hidden }
			case .archiveCategory(let id):
				try update(id) { $0.hidden = true }
			case .deleteCategory(let id):
				guard !withHistory.contains(id) else { throw FfiError.Invalid(message: "That envelope has history. Archive it instead.") }
				_ = try remove(id)
			case .deleteCategoryMovingHistory(let id, let to):
				let gone = try remove(id)
				try update(to) { $0.budgeted += gone.budgeted }
			case .moveCategory(let id, let groupID, let before):
				let moving = try remove(id)
				guard let g = groups.firstIndex(where: { $0.id == groupID }) else { throw FfiError.NotFound(entity: "group", id: groupID) }
				let at = before.flatMap { b in groups[g].envelopes.firstIndex { $0.id == b } } ?? groups[g].envelopes.count
				groups[g].envelopes.insert(moving, at: at)
			case .createCategoryGroup(let name):
				created += 1
				groups.insert(Group(id: "group-\(created)", name: name, isIncome: false, envelopes: []), at: groups.count - 1)
			case .renameCategoryGroup(let id, let name):
				if let g = groups.firstIndex(where: { $0.id == id }) { groups[g].name = name }
			case .setCategoryGroupHidden(let id, let hidden):
				if let g = groups.firstIndex(where: { $0.id == id }) { groups[g].hidden = hidden }
			case .moveCategoryGroup(let id, let before):
				guard let g = groups.firstIndex(where: { $0.id == id }) else { return }
				let moving = groups.remove(at: g)
				let at = before.flatMap { b in groups.firstIndex { $0.id == b } } ?? groups.count - 1
				groups.insert(moving, at: at)
			case .createGoal(let name, let target, let date, let icon, _):
				let icon = try StubBudget.checkedIcon(icon)
				created += 1
				goalList.insert(StubBudget.goal("goal-\(created)", name, target: target, available: 0, date: date, state: .active, monthly: date == nil ? nil : target / 6, icon: icon), at: 0)
			case .setGoalIcon(let id, let icon):
				let icon = try StubBudget.checkedIcon(icon)
				try updateGoal(id) { $0.icon = icon }
			case .closeGoal(let id, _, _):
				try updateGoal(id) { $0.state = .closed; $0.closedMonth = StubBudget.month }
			case .reopenGoal(let id):
				try updateGoal(id) { $0.state = $0.available >= $0.target ? .reached : .active; $0.closedMonth = nil }
			case .retargetGoal(let id, let target, let date):
				try updateGoal(id) {
					guard $0.state != .closed else { return }
					$0.target = target
					$0.targetDate = date
					$0.state = $0.available >= target ? .reached : .active
				}
			default:
				break
			}
		}
	}

	var goals: [GoalSummary] {
		lock.withLock { goalList }
	}

	func history(_ id: String) -> CategoryHistory {
		let bucket = taxBuckets[id]
		return withHistory.contains(id) ? CategoryHistory(transactions: 14, budgetedMonths: 6, taxBucket: bucket) : CategoryHistory(transactions: 0, budgetedMonths: 0, taxBucket: bucket)
	}

	/// A goal's leftover goes to the destination; Japan trip has later-month budget (D66).
	func closePreview(_ goalID: String, releaseTo: String?) -> GoalClosePreview {
		let sheet = month(Self.month)
		return lock.withLock {
			let goal = goalList.first { $0.id == goalID }
			let leftover = goal?.available ?? 0
			let before = releaseTo.flatMap { id in sheet.groups.flatMap(\.envelopes).first { $0.id == id }?.available } ?? sheet.toBudget
			let later = goalID == "japan" ? [MonthAmount(month: Month(year: 2026, month: 12), amount: 33_546)] : []
			return GoalClosePreview(leftover: leftover, laterBudget: later, destinationAvailableAfter: before + leftover)
		}
	}

	/// `zeroPlaces` stands for a 0-place budget, where the core keeps a
	/// negative, mixed-scale goal neutral (CLS-12.14).
	static func goal(_ id: String, _ name: String, target: Int64, available: Int64, date: String?, state: GoalState, monthly: Int64? = nil, monthsLeft: UInt32 = 5, spent: Int64 = 0, closed: Month? = nil, pace: GoalPace? = nil, icon: String? = nil, zeroPlaces: Bool = false) -> GoalSummary {
		withCoreFields(GoalSummary(
			id: id, categoryId: id, name: name, target: target, targetDate: date, icon: icon, color: nil, state: state,
			available: available, spent: spent, tone: .neutral, progressPercent: 0, progressAmount: 0,
			suggestedMonthly: monthly, monthsLeft: monthly == nil ? nil : monthsLeft,
			pace: pace, reachedMonth: nil, closedMonth: closed, laterBudget: [], newActivity: false
		), zeroPlaces: zeroPlaces)
	}

	/// The core's rules for the fields it derives (veritas-ffi goals.rs and
	/// budget.rs), so the stub answers as a real budget would.
	static func withCoreFields(_ goal: GoalSummary, zeroPlaces: Bool = false) -> GoalSummary {
		var goal = goal
		let reached = goal.state == .reached || goal.state == .reachedSpent
		goal.progressPercent = reached ? 100 : goal.available <= 0 || goal.target <= 0 ? 0 : UInt8(min(goal.available * 100 / goal.target, 99))
		goal.progressAmount = goal.state == .reachedSpent ? goal.target : goal.available
		goal.tone = switch goal.state {
		case .active where goal.available < 0, .reachedSpent where goal.available < 0: zeroPlaces ? .neutral : .overspent
		case .active:
			switch goal.pace {
			case nil, .onTrack: .available
			case .behind, .notAdding: .low
			case .noHistory, .spendingRecorded: .neutral
			}
		case .reached, .reachedSpent: .available
		case .closed: .neutral
		}
		return goal
	}

	/// The core's icon rule (v18): an SF Symbol name or one emoji; blank is none.
	static func checkedIcon(_ icon: String?) throws -> String? {
		guard let icon, !icon.isEmpty else { return nil }
		let symbol = icon.utf8.count <= 64 && !icon.hasPrefix(".") && !icon.hasSuffix(".")
			&& icon.unicodeScalars.allSatisfy { ("a"..."z").contains($0) || ("0"..."9").contains($0) || $0 == "." }
		let emoji = icon.count == 1 && icon.utf8.count <= 35 && icon.unicodeScalars.contains { $0.properties.isEmojiPresentation || $0.properties.isEmoji && !$0.isASCII }
		guard symbol || emoji else { throw FfiError.Invalid(message: "Choose a symbol or one emoji.") }
		return icon
	}

	static func toBudgetTone(_ toBudget: Int64) -> VeritasCore.MoneyTone {
		toBudget < 0 ? .overspent : toBudget == 0 ? .neutral : .income
	}

	private func updateGoal(_ id: String, _ change: (inout GoalSummary) -> Void) throws {
		guard let index = goalList.firstIndex(where: { $0.id == id }) else { throw FfiError.NotFound(entity: "goal", id: id) }
		change(&goalList[index])
		goalList[index] = Self.withCoreFields(goalList[index])
	}

	private func remove(_ id: String) throws -> Envelope {
		for g in groups.indices {
			if let e = groups[g].envelopes.firstIndex(where: { $0.id == id }) {
				return groups[g].envelopes.remove(at: e)
			}
		}
		throw FfiError.NotFound(entity: "category", id: id)
	}

	private func envelope(_ id: String) -> Envelope? {
		groups.lazy.flatMap(\.envelopes).first { $0.id == id }
	}

	private func update(_ id: String, _ change: (inout Envelope) -> Void) throws {
		for g in groups.indices {
			if let e = groups[g].envelopes.firstIndex(where: { $0.id == id }) {
				change(&groups[g].envelopes[e])
				return
			}
		}
		throw FfiError.NotFound(entity: "category", id: id)
	}

	private func available(_ envelope: Envelope) -> Int64 {
		envelope.carriedIn + envelope.budgeted + envelope.spent
	}

	private func row(_ envelope: Envelope, isIncome: Bool) -> EnvelopeRow {
		let left = available(envelope)
		let start = left - envelope.spent
		let fill = left <= 0 ? 0 : start <= 0 ? 1 : min(Double(left) / Double(start), 1)
		let tone: VeritasCore.MoneyTone = isIncome ? .income : left < 0 ? .overspent : left == 0 ? .neutral : fill <= 0.2 ? .low : .available
		return EnvelopeRow(
			id: envelope.id, name: envelope.name, hidden: envelope.hidden, budgeted: envelope.budgeted, spent: envelope.spent,
			available: isIncome ? 0 : left, carryover: envelope.carryover, tone: tone, fill: fill, note: envelope.note
		)
	}
}
#endif
