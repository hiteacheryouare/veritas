import Foundation
import Observation
import VeritasCore

/// A loan's optional terms (R-DEBT-1): rate, payment, payment day and lender.
@MainActor
@Observable
public final class LoanTermsModel: Identifiable {
	public let id = UUID()
	public var rateText: String
	public let payment: AmountFieldModel
	public var paymentDay: UInt8?
	public var lender: String
	public private(set) var problem: String?

	@ObservationIgnored private let workspace: WorkspaceModel

	public init(workspace: WorkspaceModel, terms: LoanTerms? = nil, reader: any AmountReader = CoreAmountReader()) {
		self.workspace = workspace
		let style = workspace.numberStyle
		rateText = terms?.rateBps.map { Self.rateText($0, decimalSeparator: workspace.format.decimalSeparator) } ?? ""
		payment = AmountFieldModel(amount: terms?.payment ?? 0, style: style, reader: reader)
		if terms?.payment == nil { payment.type("") }
		paymentDay = terms?.paymentDay
		lender = terms?.lender ?? ""
	}

	/// 980 basis points is "9.80".
	public nonisolated static func rateText(_ bps: Int64, decimalSeparator: String = ".") -> String {
		String(format: "%lld%@%02lld", bps / 100, decimalSeparator, abs(bps % 100))
	}

	/// What's typed as terms, or `nil` with `problem` set when a field can't be read.
	public func terms() async -> LoanTerms? {
		var terms = LoanTerms()
		let rate = rateText.trimmingCharacters(in: .whitespaces)
		if !rate.isEmpty {
			let style = workspace.numberStyle
			guard let bps = await workspace.report({ try await $0.evaluateRate(rate, style: style) }) else {
				problem = "Can't read \(rate) as a rate."
				return nil
			}
			terms.rateBps = bps
		}
		if !payment.text.trimmingCharacters(in: .whitespaces).isEmpty {
			guard let amount = await payment.commit() else {
				problem = payment.problem
				return nil
			}
			terms.payment = amount
		}
		terms.paymentDay = paymentDay
		let name = lender.trimmingCharacters(in: .whitespaces)
		terms.lender = name.isEmpty ? nil : name
		problem = nil
		return terms
	}

	@discardableResult
	public func save(to accountID: String) async -> Bool {
		guard let terms = await terms() else { return false }
		return await workspace.change { session -> Bool in
			try await session.setLoanTerms(accountID, terms)
			return true
		} ?? false
	}
}
