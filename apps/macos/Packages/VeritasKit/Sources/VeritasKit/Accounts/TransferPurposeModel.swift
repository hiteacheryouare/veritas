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

	public var answer: Answer? {
		didSet {
			guard answer != oldValue else { return }
			categoryID = target.flatMap { if case .one(let id) = $0 { id } else { nil } }
			createsEnvelope = false
			newEnvelopeName = answer == .saving ? "Savings" : "Loan Principal"
			reclassifies = nil
		}
	}
	public var categoryID: String?
	/// No envelope has the kind yet, so the answer makes one (CLS-4.1).
	public var createsEnvelope = false
	public var newEnvelopeName = ""
	/// How many earlier transactions change meaning when an existing envelope takes the kind (CLS-3.3).
	public private(set) var reclassifies: UInt64?
	public private(set) var debt: KindTarget = .none
	public private(set) var savings: KindTarget = .none
	public private(set) var envelopes: [EnvelopeGroup] = []

	@ObservationIgnored private let workspace: WorkspaceModel

	public init(workspace: WorkspaceModel, categoryID: String? = nil) {
		self.workspace = workspace
		self.categoryID = categoryID
	}

	public func load(envelopes: [EnvelopeGroup]) async {
		self.envelopes = envelopes
		debt = await workspace.report { try await $0.kindTarget(.debtPrincipal) } ?? .none
		savings = await workspace.report { try await $0.kindTarget(.savings) } ?? .none
		if answer == nil, let categoryID {
			answer = Self.answer(for: categoryID, debt: debt, savings: savings)
			self.categoryID = categoryID
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

	/// What the envelope menu offers for the answer.
	public var choices: [Choice] {
		switch target {
		case .several(let ids): all.filter { ids.contains($0.id) }.map(\.choice)
		case .one: []
		case .none?, nil: all.filter { !$0.isIncome }.map(\.choice)
		}
	}

	/// No envelope has the answer's kind, so one is chosen or made.
	public var needsKind: Bool {
		if case .none? = target { true } else { false }
	}

	public var isAnswered: Bool {
		guard answer != nil else { return false }
		if createsEnvelope { return !newEnvelopeName.trimmingCharacters(in: .whitespaces).isEmpty }
		return categoryID != nil
	}

	public func choose(_ categoryID: String?) async {
		self.categoryID = categoryID
		createsEnvelope = false
		reclassifies = nil
		guard needsKind, let categoryID, let kind = answer?.kind else { return }
		reclassifies = await workspace.report { try await $0.kindChangePreview(categoryID, kind: kind) }
	}

	public func chooseNewEnvelope() {
		categoryID = nil
		createsEnvelope = true
		reclassifies = nil
	}

	public func name(of categoryID: String) -> String? {
		all.first { $0.id == categoryID }?.name
	}

	/// The envelope to save on the on-budget side, after giving it its kind
	/// or making it. `nil` when that failed; the workspace shows why.
	func resolve(using session: any BudgetSession) async throws -> String? {
		guard let answer else { return nil }
		if createsEnvelope, let kind = answer.kind {
			return try await session.createKindCategory(kind: kind, name: newEnvelopeName.trimmingCharacters(in: .whitespaces))
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
			case .none: false
			}
		}
		if holds(debt) { return .debt }
		if holds(savings) { return .saving }
		return .other
	}
}
