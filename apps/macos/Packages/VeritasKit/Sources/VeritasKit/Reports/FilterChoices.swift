import Foundation
import Observation
import VeritasCore

/// What a report filter can point at: Actual stores an id for `is` and
/// `is not` on a category, payee or account, so those pick from a list.
@MainActor
@Observable
public final class FilterChoices {
	public struct Option: Hashable, Identifiable, Sendable {
		public let id: String
		public let name: String
		public let group: String?
	}

	public private(set) var categories: [Option] = []
	public private(set) var payees: [Option] = []
	public private(set) var accounts: [Option] = []

	@ObservationIgnored private let workspace: WorkspaceModel

	public init(workspace: WorkspaceModel) {
		self.workspace = workspace
	}

	public func load() async {
		let month = Month(date: WorkspaceModel.today()) ?? Month(year: 2026, month: 1)
		if let groups = await workspace.budgetMonth(month)?.groups {
			categories = groups.flatMap { group in group.envelopes.map { Option(id: $0.id, name: $0.name, group: group.name) } }
		}
		if let list = await workspace.report({ try await $0.accounts() }) {
			accounts = list.accounts.map { Option(id: $0.id, name: $0.name, group: nil) }
		}
		if let all = await workspace.report({ try await $0.payees() }) {
			payees = all.filter { $0.transferAccountId == nil }.map { Option(id: $0.id, name: $0.name, group: nil) }
		}
	}

	/// The list a condition picks from, or `nil` when its value is typed.
	public func options(for condition: ReportCondition) -> [Option]? {
		guard ["is", "isNot"].contains(condition.op) else { return nil }
		switch condition.field {
		case "category": return categories
		case "payee": return payees
		case "account": return accounts
		default: return nil
		}
	}
}
