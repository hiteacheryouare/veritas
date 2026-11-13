import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

struct EnvelopeGroupView: View {
	let group: EnvelopeGroup
	let screen: BudgetScreenModel
	let dense: Bool
	@State private var isAdding = false
	@State private var isRenaming = false

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
			.opacity(group.hidden ? 0.5 : 1)
			.background(dense ? AnyShapeStyle(.surfaceHover) : AnyShapeStyle(.clear))
			.contentShape(.rect)
			.contextMenu {
				if !group.isIncome {
					Button("New Envelope…") { isAdding = true }
				}
				Button("Rename Group…") { isRenaming = true }
				Button(group.hidden ? "Show Group" : "Hide Group") { Task { await screen.setGroupHidden(group.id, !group.hidden) } }
			}
			.popover(isPresented: $isAdding, arrowEdge: .bottom) {
				NamePrompt(title: "New envelope in \(group.name)", action: "Add envelope") { name in
					await screen.createEnvelope(in: group.id, named: name)
				}
			}
			.popover(isPresented: $isRenaming, arrowEdge: .bottom) {
				NamePrompt(title: "Rename group", action: "Rename", name: group.name) { name in
					await screen.renameGroup(group.id, to: name)
				}
			}
			.draggable(BudgetDrag.group(group.id)) {
				Text(group.name).font(.text(.body, weight: .semibold)).padding(Spacing.s2)
			}
			.dropDestination(for: String.self) { items, _ in
				guard let item = items.first.flatMap(BudgetDrag.parse) else { return false }
				Task {
					if item.isGroup {
						await screen.moveGroup(item.id, before: group.id)
					} else {
						await screen.moveEnvelope(item.id, to: group.id, before: group.envelopes.first?.id)
					}
				}
				return true
			}
			.accessibilityElement(children: .combine)
			.accessibilityAddTraits(.isHeader)
			.accessibilityIdentifier("group.\(group.id)")
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
			EnvelopeRowView(envelope: envelope, isIncome: group.isIncome, screen: screen, dense: dense, groupID: group.id)
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
