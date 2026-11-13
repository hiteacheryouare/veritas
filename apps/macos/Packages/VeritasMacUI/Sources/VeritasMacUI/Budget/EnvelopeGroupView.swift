import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

struct EnvelopeGroupView: View {
	let group: EnvelopeGroup
	let screen: BudgetScreenModel
	let dense: Bool

	var body: some View {
		VStack(alignment: .leading, spacing: 0) {
			EnvelopeGrid(dense: dense) {
				Text(group.name)
					.font(.text(.body, weight: .semibold))
			} bar: {
				Color.clear
			} budgeted: {
				if !group.isIncome { figure(group.budgeted) }
			} spent: {
				if !group.isIncome { figure(-group.spent) }
			} available: {
				figure(group.isIncome ? group.spent : group.available)
			}
			.frame(height: dense ? Size.rowDense : 30)
			.background(dense ? AnyShapeStyle(.surfaceHover) : AnyShapeStyle(.clear))
			.accessibilityElement(children: .combine)
			.accessibilityAddTraits(.isHeader)
			if dense {
				rows
			} else {
				GroupedRows { rows }
			}
		}
		.padding(.bottom, dense ? 0 : Spacing.s5)
	}

	/// Dense is one ledger, as in the mockup: group heads are bands inside it.
	@ViewBuilder private var rows: some View {
		ForEach(group.envelopes, id: \.id) { envelope in
			if dense { Divider() }
			EnvelopeRowView(envelope: envelope, isIncome: group.isIncome, screen: screen, dense: dense)
			if !dense, envelope.id != group.envelopes.last?.id {
				Divider().padding(.leading, Spacing.s4)
			}
		}
	}

	private func figure(_ amount: Int64) -> some View {
		Text(screen.format.amount(amount))
			.font(.figures(.body, weight: .semibold))
			.foregroundStyle(.textSecondary)
	}
}
