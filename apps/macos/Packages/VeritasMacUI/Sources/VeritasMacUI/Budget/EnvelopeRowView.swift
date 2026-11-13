import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// One envelope. Clicking Budgeted, Return, or typing a digit opens the amount
/// field; an overspent envelope's available opens Cover overspending.
struct EnvelopeRowView: View {
	let envelope: EnvelopeRow
	let isIncome: Bool
	let screen: BudgetScreenModel
	let dense: Bool
	let groupID: String
	@State private var isCovering = false
	@State private var isEditingNote = false
	@State private var isRenaming = false
	@State private var removal: BudgetScreenModel.Removal?
	@State private var isMovingHistory = false
	@Environment(\.accessibilityReduceMotion) private var reduceMotion

	private var isSelected: Bool { screen.selection == envelope.id }
	private var field: AmountFieldModel? { screen.editingID == envelope.id ? screen.editing : nil }

	var body: some View {
		EnvelopeGrid(dense: dense) {
			HStack(spacing: Spacing.s2) {
				Text(envelope.name)
					.lineLimit(1)
				if let note = envelope.note {
					Image(systemName: "text.alignleft")
						.imageScale(.small)
						.foregroundStyle(.textTertiary)
						.help(note)
						.accessibilityLabel("Note: \(note)")
				}
			}
		} bar: {
			if let field {
				EditPreview(field: field, format: screen.format)
			} else if !isIncome {
				DrainBar(fill: envelope.fill, tone: envelope.tone.design)
			}
		} budgeted: {
			budgeted
		} spent: {
			if !isIncome {
				Text(screen.format.amount(-envelope.spent))
					.font(.figures(.body))
					.foregroundStyle(envelope.spent == 0 ? Color(ColorToken.moneyNeutral) : Color.textPrimary)
			}
		} available: {
			available
		}
		.frame(height: dense ? Size.rowDense : Size.row)
		.opacity(envelope.hidden ? 0.5 : 1)
		.background(isSelected || field != nil ? AnyShapeStyle(.accentSoft) : AnyShapeStyle(.clear))
		.motion(.fast, value: isSelected)
		.contentShape(.rect)
		.onTapGesture { screen.selection = envelope.id }
		.contextMenu { menu }
		.popover(isPresented: $isEditingNote, arrowEdge: .trailing) {
			NotePopover(screen: screen, envelope: envelope)
		}
		.popover(isPresented: $isRenaming, arrowEdge: .bottom) {
			NamePrompt(title: "Rename envelope", action: "Rename", name: envelope.name) { name in
				await screen.renameEnvelope(envelope.id, to: name)
			}
		}
		.confirmationDialog("Delete \(envelope.name)?", isPresented: Binding(get: { removal == .delete }, set: { if !$0 { removal = nil } })) {
			Button("Delete", role: .destructive) { Task { await screen.delete(envelope.id) } }
		} message: {
			Text("It has no transactions or budget.")
		}
		.alert(archiveTitle, isPresented: Binding(get: { isArchive }, set: { if !$0 { removal = nil } })) {
			Button("Archive") { Task { await screen.archive(envelope.id) } }
				.keyboardShortcut(.defaultAction)
			Button("Delete and Move History…", role: .destructive) { isMovingHistory = true }
			Button("Cancel", role: .cancel) {}
		} message: {
			Text(archiveMessage)
		}
		.sheet(isPresented: $isMovingHistory) {
			if case .archive(let transactions, let months, let taxBucket) = archivedHistory {
				DeleteMovingHistorySheet(screen: screen, envelope: envelope, transactions: transactions, budgetedMonths: months, taxBucket: taxBucket)
			}
		}
		.draggable(BudgetDrag.envelope(envelope.id)) {
			Text(envelope.name).padding(Spacing.s2)
		}
		.dropDestination(for: String.self) { items, _ in
			guard let item = items.first.flatMap(BudgetDrag.parse), !item.isGroup else { return false }
			Task { await screen.moveEnvelope(item.id, to: groupID, before: envelope.id) }
			return true
		}
		.accessibilityElement(children: .contain)
		.accessibilityLabel(Self.spoken(envelope, isIncome: isIncome, format: screen.format))
		.accessibilityAddTraits(isSelected ? .isSelected : [])
		.containerIdentifier("envelope.\(envelope.id)")
	}

	@ViewBuilder private var budgeted: some View {
		if let field {
			VStack(alignment: .trailing, spacing: 2) {
				AmountTextField(field: field) { exit in
					switch exit {
					case .next: Task { await screen.commitEditing(thenMove: 1) }
					case .previous: Task { await screen.commitEditing(thenMove: -1) }
					case .done: Task { await screen.commitEditing() }
					case .cancel: screen.cancelEditing()
					}
				}
				.frame(width: 110, height: 26)
			}
		} else if isIncome {
			Color.clear
		} else {
			Button {
				screen.beginEditing(envelope.id)
			} label: {
				Text(screen.format.amount(envelope.budgeted))
					.font(.figures(.body))
					.frame(maxWidth: .infinity, alignment: .trailing)
					.contentShape(.rect)
			}
			.buttonStyle(.plain)
			.disabled(screen.isReadOnly)
			.accessibilityLabel("Budgeted \(screen.format.amount(envelope.budgeted))")
			.accessibilityIdentifier("envelope.\(envelope.id).budgeted")
		}
	}

	@ViewBuilder private var available: some View {
		if isIncome {
			AvailableLabel(amount: envelope.spent, tone: .income, format: screen.format)
		} else if envelope.tone == .overspent {
			Button {
				isCovering = true
			} label: {
				AvailableLabel(amount: envelope.available, tone: .overspent, format: screen.format)
			}
			.buttonStyle(.plain)
			.disabled(screen.isReadOnly)
			.help("Cover overspending")
			.popover(isPresented: $isCovering, arrowEdge: .bottom) {
				CoverPopover(screen: screen, envelope: envelope)
			}
			.accessibilityIdentifier("envelope.\(envelope.id).cover")
		} else {
			AvailableLabel(amount: envelope.available, tone: envelope.tone.design, format: screen.format)
		}
	}

	@ViewBuilder private var menu: some View {
		if !isIncome {
			Button("Edit Budgeted Amount") { screen.beginEditing(envelope.id) }
			if envelope.tone == .overspent {
				Button("Cover Overspending…") { isCovering = true }
			}
			Toggle("Roll Over Overspending", isOn: Binding(get: { envelope.carryover }, set: { value in
				Task { await screen.setCarryover(envelope.id, value) }
			}))
			Divider()
		}
		Button(envelope.note == nil ? "Add Note…" : "Edit Note…") { isEditingNote = true }
		Divider()
		Button("Move Up") { Task { await screen.moveEnvelope(envelope.id, by: -1) } }
			.disabled(!screen.canMoveEnvelope(envelope.id, by: -1))
		Button("Move Down") { Task { await screen.moveEnvelope(envelope.id, by: 1) } }
			.disabled(!screen.canMoveEnvelope(envelope.id, by: 1))
		Divider()
		Button("Rename…") { isRenaming = true }
		Button(envelope.hidden ? "Show" : "Hide") { Task { await screen.setHidden(envelope.id, !envelope.hidden) } }
		Button("Delete…") {
			Task {
				removal = await screen.removal(for: envelope.id)
				if case .archive = removal { archivedHistory = removal }
			}
		}
	}

	@State private var archivedHistory: BudgetScreenModel.Removal?

	private var isArchive: Bool {
		if case .archive = removal { return true }
		return false
	}

	private var archiveTitle: String {
		"Archive \(envelope.name)?"
	}

	/// R-BUD-5: deleting an envelope with history archives it instead.
	private var archiveMessage: String {
		guard case .archive(let transactions, let months, _) = removal else { return "" }
		return "It has \(DeleteMovingHistorySheet.count(transactions, "transaction")) and budget in \(DeleteMovingHistorySheet.count(months, "month")). Archive hides it from the budget."
	}

	/// design/interaction.md: rows read as a sentence.
	static func spoken(_ envelope: EnvelopeRow, isIncome: Bool, format: MoneyFormat) -> String {
		if isIncome {
			return "\(envelope.name), received \(format.amount(envelope.spent))"
		}
		let state = switch envelope.tone {
		case .overspent: "overspent by \(format.amount(-envelope.available))"
		case .low: "low, \(format.amount(envelope.available)) left"
		case .neutral: "nothing left"
		default: "\(format.amount(envelope.available)) left"
		}
		return "\(envelope.name), budgeted \(format.amount(envelope.budgeted)), spent \(format.amount(-envelope.spent)), \(state)"
	}
}

/// "Return sets 452.00" while the field holds an expression.
private struct EditPreview: View {
	let field: AmountFieldModel
	let format: MoneyFormat

	var body: some View {
		Group {
			if let problem = field.problem {
				ProblemNote(text: problem)
					.lineLimit(2)
			} else if let preview = field.preview {
				HStack(spacing: 4) {
					Text("Return sets")
						.font(.text(.caption))
						.foregroundStyle(.textSecondary)
					Text(format.amount(preview))
						.font(.figures(.body, weight: .semibold))
				}
				.accessibilityElement(children: .combine)
				.accessibilityIdentifier("budget.editPreview")
			}
		}
		.frame(maxWidth: .infinity, alignment: .trailing)
	}
}
