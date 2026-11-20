import Foundation
import VeritasCore

/// Names for the ids bills and rules carry, and the choices their pickers offer.
public struct BudgetNames: Hashable, Sendable {
	public let payees: [PayeeDetail]
	public let accounts: [AccountDetail]
	public let categoryGroups: [EnvelopeGroup]

	public static let empty = BudgetNames(payees: [], accounts: [], categoryGroups: [])

	public init(payees: [PayeeDetail], accounts: [AccountDetail], categoryGroups: [EnvelopeGroup]) {
		self.payees = payees
		self.accounts = accounts
		self.categoryGroups = categoryGroups
	}

	public var openAccounts: [AccountDetail] { accounts.filter { !$0.closed } }

	public func payee(_ id: String?) -> String? { id.flatMap { id in payees.first { $0.id == id }?.name } }
	public func account(_ id: String?) -> String? { id.flatMap { id in accounts.first { $0.id == id }?.name } }
	public func categoryGroup(_ id: String?) -> String? { id.flatMap { id in categoryGroups.first { $0.id == id }?.name } }

	public func category(_ id: String?) -> String? {
		guard let id else { return nil }
		for group in categoryGroups {
			if let envelope = group.envelopes.first(where: { $0.id == id }) { return envelope.name }
		}
		return nil
	}
}

public extension WorkspaceModel {
	/// Payees, accounts and this month's category groups, for pickers and sentences.
	func budgetNames() async -> BudgetNames {
		let today = Self.today()
		guard let month = Month(date: today) else { return .empty }
		return await report { session in
			async let payees = session.payees()
			async let accounts = session.accounts()
			async let budget = session.budgetMonth(month, today: today)
			return try await BudgetNames(payees: payees, accounts: accounts.accounts, categoryGroups: budget.groups)
		} ?? .empty
	}
}
