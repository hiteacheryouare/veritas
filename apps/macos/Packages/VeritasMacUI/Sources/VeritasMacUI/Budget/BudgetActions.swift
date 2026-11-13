import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Move budget between envelopes, or to and from To Budget (R-BUD-4). It's a
/// budget move, never a transaction.
struct MoveBudgetSheet: View {
	let screen: BudgetScreenModel
	@State private var from: String?
	@State private var to: String?
	@State private var field: AmountFieldModel
	@Environment(\.dismiss) private var dismiss

	init(screen: BudgetScreenModel, from: String?) {
		self.screen = screen
		let start = from.flatMap { id in screen.spendingEnvelopes.first { $0.id == id } }
		_from = State(initialValue: start?.id)
		_field = State(initialValue: AmountFieldModel(amount: max(start?.available ?? 0, 0), style: screen.numberStyle, reader: screen.reader))
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s5) {
			Text("Move budget")
				.font(.text(.title, weight: .semibold))
			GroupedRows {
				LabeledRow("From") { EnvelopePicker(title: "From", selection: $from, envelopes: screen.spendingEnvelopes, format: screen.format) }
				Divider().padding(.leading, Spacing.s4)
				LabeledRow("To") { EnvelopePicker(title: "To", selection: $to, envelopes: screen.spendingEnvelopes, format: screen.format) }
				Divider().padding(.leading, Spacing.s4)
				LabeledRow("Amount") {
					AmountTextField(field: field) { exit in
						switch exit {
						case .cancel: dismiss()
						default: move()
						}
					}
					.frame(width: 140, height: 26)
				}
			}
			if let problem = field.problem ?? sameEnvelope {
				ProblemNote(text: problem)
			}
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				Button("Move budget", action: move)
					.buttonStyle(.borderedProminent)
					.keyboardShortcut(.defaultAction)
					.disabled(sameEnvelope != nil)
					.accessibilityIdentifier("moveBudget.move")
			}
		}
		.padding(Spacing.s5)
		.frame(width: 480)
		.containerIdentifier("sheet.moveBudget")
	}

	private var sameEnvelope: String? {
		from == to ? "Choose two different places." : nil
	}

	private func move() {
		guard sameEnvelope == nil else { return }
		Task {
			guard let amount = await field.commit() else { return }
			if await screen.moveBudget(amount: amount, from: from, to: to) {
				dismiss()
			}
		}
	}
}

/// To Budget, then every envelope with what it has available.
struct EnvelopePicker: View {
	let title: String
	@Binding var selection: String?
	let envelopes: [EnvelopeRow]
	let format: MoneyFormat

	var body: some View {
		Picker(title, selection: $selection) {
			Text("To Budget").tag(String?.none)
			Divider()
			ForEach(envelopes, id: \.id) { envelope in
				Text("\(envelope.name)  \(format.amount(envelope.available))").tag(String?.some(envelope.id))
			}
		}
		.labelsHidden()
		.frame(width: 240)
		.accessibilityIdentifier("moveBudget.\(title.lowercased())")
	}
}

/// Cover overspending from To Budget or an envelope with money left (R-BUD-4).
struct CoverPopover: View {
	let screen: BudgetScreenModel
	let envelope: EnvelopeRow
	@State private var from: String?
	@Environment(\.dismiss) private var dismiss

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s3) {
			Text("Cover \(screen.format.amount(-envelope.available)) overspent")
				.font(.text(.heading, weight: .semibold))
			EnvelopePicker(title: "From", selection: $from, envelopes: screen.sources.filter { $0.id != envelope.id }, format: screen.format)
			HStack {
				Spacer()
				Button("Cover") {
					Task {
						if await screen.coverOverspending(envelope.id, from: from) { dismiss() }
					}
				}
				.buttonStyle(.borderedProminent)
				.keyboardShortcut(.defaultAction)
				.accessibilityIdentifier("cover.cover")
			}
		}
		.padding(Spacing.s4)
		.frame(width: 300)
		.containerIdentifier("popover.cover")
	}
}

struct HoldPopover: View {
	let screen: BudgetScreenModel
	@State private var field: AmountFieldModel
	@Environment(\.dismiss) private var dismiss

	init(screen: BudgetScreenModel, toBudget: Int64) {
		self.screen = screen
		_field = State(initialValue: AmountFieldModel(amount: toBudget, style: screen.numberStyle, reader: screen.reader))
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s3) {
			Text("Hold for next month")
				.font(.text(.heading, weight: .semibold))
			AmountTextField(field: field) { exit in
				if exit == .cancel { dismiss() } else { hold() }
			}
			.frame(width: 160, height: 26)
			if let problem = field.problem {
				ProblemNote(text: problem)
			}
			HStack {
				Spacer()
				Button("Hold", action: hold)
					.buttonStyle(.borderedProminent)
					.keyboardShortcut(.defaultAction)
			}
		}
		.padding(Spacing.s4)
		.frame(width: 260)
	}

	private func hold() {
		Task {
			guard let amount = await field.commit() else { return }
			if await screen.holdForNextMonth(amount) { dismiss() }
		}
	}
}

/// A category's note (R-BUD-6). Clearing it removes the note.
struct NotePopover: View {
	let screen: BudgetScreenModel
	let envelope: EnvelopeRow
	@State private var text: String
	@Environment(\.dismiss) private var dismiss

	init(screen: BudgetScreenModel, envelope: EnvelopeRow) {
		self.screen = screen
		self.envelope = envelope
		_text = State(initialValue: envelope.note ?? "")
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s3) {
			Text(envelope.name)
				.font(.text(.heading, weight: .semibold))
			TextEditor(text: $text)
				.font(.text(.body))
				.frame(width: 280, height: 100)
				.accessibilityIdentifier("note.text")
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				Button("Save note") {
					Task {
						await screen.setNote(envelope.id, text)
						dismiss()
					}
				}
				.buttonStyle(.borderedProminent)
				.accessibilityIdentifier("note.save")
			}
		}
		.padding(Spacing.s4)
	}
}
