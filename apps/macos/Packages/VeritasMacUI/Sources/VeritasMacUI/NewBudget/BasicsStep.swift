import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

struct BasicsStep: View {
	@Bindable var setup: NewBudgetSetup
	@FocusState private var isNameFocused: Bool

	var body: some View {
		GroupedRows {
			LabeledRow("Name") {
				TextField("Name", text: $setup.name)
					.textFieldStyle(.roundedBorder)
					.labelsHidden()
					.frame(width: 260)
					.focused($isNameFocused)
					.accessibilityIdentifier("newBudget.name")
			}
			Divider().padding(.leading, Spacing.s4)
			LabeledRow("Currency") {
				Picker("Currency", selection: Binding(get: { setup.currencyCode }, set: setup.choose(currency:))) {
					ForEach(setup.currencies) { currency in
						Text("\(currency.name) (\(currency.code))").tag(currency.code)
					}
				}
				.labelsHidden()
				.fixedSize()
				.disabled(setup.currencies.isEmpty)
				.accessibilityIdentifier("newBudget.currency")
			}
			Divider().padding(.leading, Spacing.s4)
			LabeledRow("Number format") {
				Picker("Number format", selection: $setup.numberStyle) {
					ForEach(NumberStyle.allCases, id: \.self) { style in
						Text(style.sample).tag(style)
					}
				}
				.labelsHidden()
				.fixedSize()
				.font(.figures(.body))
				.accessibilityIdentifier("newBudget.numberFormat")
			}
		}
		.defaultFocus($isNameFocused, true)
	}
}
