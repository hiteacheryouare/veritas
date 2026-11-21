import Foundation
import Observation
import VeritasCore

/// R-XFER-4's question for money leaving the budget, and the envelope its
/// answer files the on-budget side under (CLS-4.1).
@MainActor
@Observable
public final class TransferPurposeModel {
	public enum Answer: String, CaseIterable, Hashable, Sendable {
		case debt
		case saving
		case other

		public var title: String {
			switch self {
			case .debt: "Loan or debt payment"
			case .saving: "Saving or investing"
			case .other: "Something else"
			}
		}

		var kind: CategoryKind? {
			switch self {
			case .debt: .debtPrincipal
			case .saving: .savings
			case .other: nil
			}
		}
	}

	public struct Choice: Hashable, Identifiable, Sendable {
		public let id: String
		public let name: String
		public let group: String
	}

	/// What saving asks the user to agree to first (CLS-4.1).
	public struct Confirmation: Hashable, Sendable {
		public let title: String
		public let message: String?
		public let action: String
	}

	public var answer: Answer? {
		didSet {
			guard answer != oldValue else { return }
			switch target {
			case .one(let id)?: select(id)
			case .oneHidden(let match)?, .oneArchived(let match)?: select(match.categoryId)
			default: select(nil)
			}
			createsEnvelope = false
			newEnvelopeName = answer == .saving ? "Savings" : "Loan Principal"
			reclassifies = nil
		}
	}
	public private(set) var categoryID: String?
	/// The hidden or archived envelope the answer shows again instead of making
	/// a second one (CLS-4.1). Saving revives it through the core.
	public private(set) var revival: HiddenCategoryMatch?
	/// No envelope has the kind yet, so the answer makes one (CLS-4.1).
	public var createsEnvelope = false
	public var newEnvelopeName = ""
	/// How many earlier transactions change meaning when an existing envelope takes the kind (CLS-3.3).
	public private(set) var reclassifies: UInt64?
	public private(set) var debt: KindTarget = .none
	public private(set) var savings: KindTarget = .none
	public private(set) var envelopes: [EnvelopeGroup] = []
	/// Where a new envelope would go (CLS-4.1).
	public private(set) var plan: KindCategoryPlan?

	@ObservationIgnored private let workspace: WorkspaceModel

	public init(workspace: WorkspaceModel, categoryID: String? = nil) {
		self.workspace = workspace
		self.categoryID = categoryID
	}

	/// The envelope the transfer is already filed under, when editing one.
	public func restore(_ categoryID: String) {
		select(categoryID)
	}

	private func select(_ categoryID: String?) {
		self.categoryID = categoryID
		revival = revivals.first { $0.categoryId == categoryID }
	}

	public func load(envelopes: [EnvelopeGroup]) async {
		self.envelopes = envelopes
		debt = await workspace.report { try await $0.kindTarget(.debtPrincipal) } ?? .none
		savings = await workspace.report { try await $0.kindTarget(.savings) } ?? .none
		if answer == nil, let categoryID {
			answer = Self.answer(for: categoryID, debt: debt, savings: savings)
			select(categoryID)
		}
	}

	var target: KindTarget? {
		switch answer {
		case .debt: debt
		case .saving: savings
		case .other, nil: nil
		}
	}

	/// The envelope the answer files under, when there's only one.
	public var fixedEnvelope: Choice? {
		guard case .one(let id) = target else { return nil }
		return all.first { $0.id == id }?.choice
	}

	public var fixedDebtEnvelope: String? {
		if case .one(let id) = debt { id } else { nil }
	}

	/// What the envelope menu offers for the answer, besides `revivals`.
	public var choices: [Choice] {
		switch target {
		case .several(let ids): all.filter { ids.contains($0.id) }.map(\.choice)
		case .one: []
		case .none?, .oneHidden?, .oneArchived?, .severalHidden?, nil:
			all.filter { listed in !listed.isIncome && !revivals.contains { $0.categoryId == listed.id } }.map(\.choice)
		}
	}

	/// The hidden and archived envelopes of the answer's kind, offered first
	/// when no visible one has it (CLS-4.1).
	public var revivals: [HiddenCategoryMatch] {
		switch target {
		case .oneHidden(let match)?, .oneArchived(let match)?: [match]
		case .severalHidden(let matches)?: matches
		case .none?, .one?, .several?, nil: []
		}
	}

	/// "Old Car Loan in Debt (hidden)", as the envelope menu lists a revival (CLS-4.1).
	public static func label(_ match: HiddenCategoryMatch) -> String {
		"\(match.name) in \(match.groupName) (\(match.archived ? "archived" : "hidden"))"
	}

	/// A lone hidden envelope is shown again rather than duplicated. An
	/// archived one may be replaced under a free name (CLS-4.1).
	public var canCreateEnvelope: Bool {
		switch target {
		case .none?, .oneArchived?, .severalHidden?: true
		case .oneHidden?, .one?, .several?, nil: false
		}
	}

	/// No visible envelope has the answer's kind, so one is chosen, shown
	/// again or made.
	public var needsKind: Bool {
		switch target {
		case .none?, .oneHidden?, .oneArchived?, .severalHidden?: true
		case .one?, .several?, nil: false
		}
	}

	public var isAnswered: Bool {
		guard answer != nil else { return false }
		if createsEnvelope { return !newEnvelopeName.trimmingCharacters(in: .whitespaces).isEmpty }
		return categoryID != nil
	}

	public func choose(_ categoryID: String?) async {
		select(categoryID)
		createsEnvelope = false
		reclassifies = nil
		guard needsKind, revival == nil, let categoryID, let kind = answer?.kind else { return }
		reclassifies = await workspace.report { try await $0.kindChangePreview(categoryID, kind: kind) }
	}

	public func chooseNewEnvelope() async {
		select(nil)
		createsEnvelope = true
		reclassifies = nil
		guard let kind = answer?.kind else { return }
		plan = await workspace.report { try await $0.kindCategoryPlan(kind) }
	}

	/// Asked before saving when the new envelope's group is hidden; the
	/// answer is the only way it's shown again.
	public var unhidePrompt: String? {
		guard createsEnvelope, let plan, plan.groupIsHidden else { return nil }
		let name = newEnvelopeName.trimmingCharacters(in: .whitespaces)
		return "Your group \"\(plan.groupName)\" is hidden. Show it again and add \"\(name)\" to it?"
	}

	/// Asked before saving: reviving the chosen envelope, or showing the new
	/// one's hidden group. Cancel writes nothing.
	public var confirmation: Confirmation? {
		if let revival {
			let title = revival.archived
				? "Restore \"\(revival.name)\" and use it?"
				: "Show \"\(revival.name)\" again and use it?"
			let message = revival.groupIsHidden ? "Its group \"\(revival.groupName)\" is hidden and will be shown again too." : nil
			return Confirmation(title: title, message: message, action: revival.archived ? "Restore and Use" : "Show and Use")
		}
		return unhidePrompt.map { Confirmation(title: $0, message: nil, action: "Show Group and Add") }
	}

	public func name(of categoryID: String) -> String? {
		all.first { $0.id == categoryID }?.name
	}

	/// The envelope to save on the on-budget side, after giving it its kind
	/// or making it. `nil` when that failed; the workspace shows why.
	func resolve(using session: any BudgetSession) async throws -> String? {
		guard let answer else { return nil }
		if createsEnvelope, let kind = answer.kind {
			return try await session.createKindCategory(kind: kind, name: newEnvelopeName.trimmingCharacters(in: .whitespaces), unhideGroup: unhidePrompt != nil)
		}
		if let revival, let kind = answer.kind {
			try await session.reviveKindCategory(kind: kind, categoryID: revival.categoryId)
			return revival.categoryId
		}
		if needsKind, let kind = answer.kind, let categoryID {
			try await session.setCategoryKind(categoryID, kind: kind)
		}
		return categoryID
	}

	private struct Listed {
		let id: String
		let name: String
		let group: String
		let isIncome: Bool
		var choice: Choice { Choice(id: id, name: name, group: group) }
	}

	private var all: [Listed] {
		envelopes.filter { !$0.hidden }.flatMap { group in
			group.envelopes.filter { !$0.hidden }.map { Listed(id: $0.id, name: $0.name, group: group.name, isIncome: group.isIncome) }
		}
	}

	private static func answer(for categoryID: String, debt: KindTarget, savings: KindTarget) -> Answer {
		func holds(_ target: KindTarget) -> Bool {
			switch target {
			case .one(let id): id == categoryID
			case .several(let ids): ids.contains(categoryID)
			case .none, .oneHidden, .oneArchived, .severalHidden: false
			}
		}
		if holds(debt) { return .debt }
		if holds(savings) { return .saving }
		return .other
	}
}
