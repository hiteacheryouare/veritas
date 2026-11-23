import Foundation
import Observation
import VeritasCore

/// The tour's stops, in order (D79): the Budget grid, then Accounts and a register.
public enum TourStep: String, CaseIterable, Sendable {
	case toBudget
	case budgetCell
	case openAccount
	case addTransaction
	case quickEntry
}

/// What the open budget has for the tour to point at. A step with nothing
/// to point at is left out.
public struct TourBudget: Hashable, Sendable {
	public var hasToBudget: Bool
	public var hasBudgetCell: Bool
	/// The register the account steps open.
	public var accountID: String?

	public init(hasToBudget: Bool, hasBudgetCell: Bool, accountID: String?) {
		self.hasToBudget = hasToBudget
		self.hasBudgetCell = hasBudgetCell
		self.accountID = accountID
	}
}

/// The first-launch coach-mark tour: its steps, where it is, and whether it
/// has been skipped or finished. That last part is an app preference, never
/// part of a budget.
@MainActor
@Observable
public final class GuidedTour {
	public static let finishedKey = "tourFinished"

	public private(set) var steps: [TourStep] = []
	public private(set) var index: Int?
	public private(set) var accountID: String?

	@ObservationIgnored private let defaults: UserDefaults

	public init(defaults: UserDefaults = .standard) {
		self.defaults = defaults
	}

	/// Read each time, so a tour finished in another window counts here too.
	public var isFinished: Bool { defaults.bool(forKey: Self.finishedKey) }

	public var current: TourStep? { index.map { steps[$0] } }

	/// One-based, for "2 of 5".
	public var number: Int? { index.map { $0 + 1 } }

	public var isLastStep: Bool { index == steps.count - 1 }

	public var canGoBack: Bool { (index ?? 0) > 0 }

	public var destination: Destination? {
		switch current {
		case .toBudget, .budgetCell: .screen(.budget)
		case .openAccount: .screen(.accounts)
		case .addTransaction, .quickEntry: accountID.map { .account(id: $0) }
		case nil: nil
		}
	}

	public static func steps(for budget: TourBudget) -> [TourStep] {
		TourStep.allCases.filter { step in
			switch step {
			case .toBudget: budget.hasToBudget
			case .budgetCell: budget.hasBudgetCell
			case .openAccount, .addTransaction, .quickEntry: budget.accountID != nil
			}
		}
	}

	/// After a budget opens. Runs until it is skipped or finished once.
	@discardableResult
	public func offer(_ budget: TourBudget) -> Bool {
		guard !isFinished, index == nil else { return false }
		return start(budget)
	}

	/// Help › Show Tour: from the first step, finished or not.
	@discardableResult
	public func replay(_ budget: TourBudget) -> Bool {
		start(budget)
	}

	public func next() {
		guard let index else { return }
		if index + 1 < steps.count {
			self.index = index + 1
		} else {
			finish()
		}
	}

	public func back() {
		guard let index, index > 0 else { return }
		self.index = index - 1
	}

	public func skip() {
		finish()
	}

	/// Leaves the tour without finishing it, as when its budget closes.
	public func stop() {
		index = nil
	}

	/// Opening an account while the tour asks for one moves on, to that account.
	public func follow(_ destination: Destination) {
		guard current == .openAccount, case .account(let id) = destination else { return }
		accountID = id
		next()
	}

	private func start(_ budget: TourBudget) -> Bool {
		steps = Self.steps(for: budget)
		accountID = budget.accountID
		index = steps.isEmpty ? nil : 0
		return index != nil
	}

	private func finish() {
		index = nil
		defaults.set(true, forKey: Self.finishedKey)
	}
}

extension WorkspaceModel {
	/// What the tour can point at, from this month's sheet and the sidebar's
	/// accounts, matching what the Budget and Accounts screens show.
	public func tourBudget() async -> TourBudget {
		let sheet = await budgetMonth(BudgetScreenModel.currentMonth())
		let hasCell = sheet?.groups.contains { group in
			!group.hidden && !group.isIncome && group.envelopes.contains { !$0.hidden }
		} ?? false
		return TourBudget(
			hasToBudget: sheet != nil && readOnly != .trackingBudget,
			hasBudgetCell: hasCell,
			accountID: overview?.accounts.first { !$0.isInactive }?.id
		)
	}
}
