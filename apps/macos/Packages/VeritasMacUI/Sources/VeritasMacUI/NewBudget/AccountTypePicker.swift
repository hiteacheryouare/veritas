import SwiftUI
import VeritasCore
import VeritasKit

/// Grouped the way the sidebar groups accounts, so the choice shows where the
/// account will appear. The core turns the type into off-budget and account kind.
struct AccountTypePicker: View {
	@Binding var type: NewAccountKind

	var body: some View {
		Picker("Type", selection: $type) {
			ForEach(VeritasCore.AccountGroup.allCases, id: \.self) { group in
				Section(group.title) {
					ForEach(NewAccountKind.allCases.filter { $0.group == group }, id: \.self) { type in
						Text(type.title).tag(type)
					}
				}
			}
		}
		.labelsHidden()
		.fixedSize()
		.accessibilityIdentifier("newBudget.account.type")
	}
}
