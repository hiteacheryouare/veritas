import SwiftUI
import VeritasDesign
import VeritasKit

/// A loan's terms (R-DEBT-1), each optional.
struct LoanTermsRows: View {
	@Bindable var terms: LoanTermsModel

	var body: some View {
		GroupedRows {
			LabeledRow("Interest rate") {
				HStack(spacing: Spacing.s1) {
					TextField("Rate", text: $terms.rateText)
						.labelsHidden()
						.multilineTextAlignment(.trailing)
						.textFieldStyle(.plain)
						.font(.figures(.body))
						.frame(width: 80)
						.accessibilityIdentifier("loan.rate")
					Text("%").foregroundStyle(.textSecondary)
				}
			}
			Divider()
			LabeledRow("Payment") {
				AmountTextField(field: terms.payment, takesFocus: false) { _ in }
					.frame(width: 120)
					.accessibilityIdentifier("loan.payment")
			}
			Divider()
			LabeledRow("Payment day") {
				Picker("Payment day", selection: $terms.paymentDay) {
					Text("None").tag(UInt8?.none)
					ForEach(1...31, id: \.self) { day in Text("\(day)").tag(UInt8?.some(UInt8(day))) }
				}
				.labelsHidden()
				.frame(width: 100)
				.accessibilityIdentifier("loan.day")
			}
			Divider()
			LabeledRow("Lender") {
				TextField("Lender", text: $terms.lender)
					.labelsHidden()
					.multilineTextAlignment(.trailing)
					.textFieldStyle(.plain)
					.frame(width: 220)
					.accessibilityIdentifier("loan.lender")
			}
		}
		.containerIdentifier("loan.terms")
	}
}

struct LoanTermsSheet: View {
	let name: String
	let terms: LoanTermsModel
	let save: () async -> Bool
	@Environment(\.dismiss) private var dismiss

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			Text(name).font(.text(.title, weight: .semibold))
			LoanTermsRows(terms: terms)
			if let problem = terms.problem {
				Text(problem).foregroundStyle(Color.moneyOverspent)
			}
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				Button("Save") { Task { if await save() { dismiss() } } }
					.buttonStyle(.borderedProminent)
					.keyboardShortcut(.defaultAction)
					.accessibilityIdentifier("loan.save")
			}
		}
		.padding(Spacing.s5)
		.frame(width: 460)
		.containerIdentifier("sheet.loanTerms")
	}
}
