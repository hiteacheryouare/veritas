#if DEBUG
import Foundation
import VeritasCore
import VeritasKit

/// The approved mockup's November 2026, kept in memory so UI tests can budget,
/// move and cover against known numbers. Only debug builds have it.
final class StubBudget: @unchecked Sendable {
	struct Envelope {
		let id: String
		let name: String
		var budgeted: Int64
		let spent: Int64
		let carriedIn: Int64
		var note: String?
		var carryover = false
	}

	struct Group {
		let id: String
		let name: String
		let isIncome: Bool
		var envelopes: [Envelope]
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
			Envelope(id: "internet", name: "Internet", budgeted: 7_000, spent: -7_000, carriedIn: 0)
		]),
		Group(id: "personal", name: "Personal", isIncome: false, envelopes: [
			Envelope(id: "hobbies", name: "Hobbies", budgeted: 7_500, spent: -1_299, carriedIn: 0),
			Envelope(id: "gifts", name: "Gifts", budgeted: 4_000, spent: 0, carriedIn: 8_000, note: "Mom's birthday in December")
		]),
		Group(id: "income", name: "Income", isIncome: true, envelopes: [
			Envelope(id: "paycheck", name: "Paycheck", budgeted: 0, spent: 512_640, carriedIn: 0)
		])
	]
	private var held: Int64 = 0

	func month(_ month: Month) -> BudgetMonth {
		lock.withLock {
			let rows = groups.map { group in
				EnvelopeGroup(
					id: group.id, name: group.name, isIncome: group.isIncome, hidden: false,
					budgeted: group.envelopes.reduce(0) { $0 + $1.budgeted },
					spent: group.envelopes.reduce(0) { $0 + $1.spent },
					available: group.isIncome ? 0 : group.envelopes.reduce(0) { $0 + available($1) },
					envelopes: group.envelopes.map { row($0, isIncome: group.isIncome) }
				)
			}
			let income = groups.filter(\.isIncome).flatMap(\.envelopes).reduce(0) { $0 + $1.spent }
			let budgeted = groups.filter { !$0.isIncome }.flatMap(\.envelopes).reduce(0) { $0 + $1.budgeted }
			return BudgetMonth(
				month: Self.month, firstMonth: Month(year: 2026, month: 1), lastMonth: Month(year: 2026, month: 12),
				toBudget: income - budgeted - held, income: income, fromLastMonth: 0, overspentLastMonth: 0,
				budgeted: budgeted, forNextMonth: held, spent: 0, available: 0, groups: rows
			)
		}
	}

	func perform(_ command: Command) throws {
		try lock.withLock {
			switch command {
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
			default:
				break
			}
		}
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
			id: envelope.id, name: envelope.name, hidden: false, budgeted: envelope.budgeted, spent: envelope.spent,
			available: isIncome ? 0 : left, carryover: envelope.carryover, tone: tone, fill: fill, note: envelope.note
		)
	}
}
#endif
