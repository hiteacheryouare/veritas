import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// A name for a new or renamed envelope or group.
struct NamePrompt: View {
	let title: String
	let action: String
	let commit: (String) async -> Bool
	@State private var name: String
	@Environment(\.dismiss) private var dismiss

	init(title: String, action: String, name: String = "", commit: @escaping (String) async -> Bool) {
		self.title = title
		self.action = action
		self.commit = commit
		_name = State(initialValue: name)
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s3) {
			Text(title)
				.font(.text(.heading, weight: .semibold))
			TextField("Name", text: $name)
				.textFieldStyle(.roundedBorder)
				.frame(width: 240)
				.onSubmit(save)
				.accessibilityIdentifier("namePrompt.name")
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				Button(action, action: save)
					.buttonStyle(.borderedProminent)
					.keyboardShortcut(.defaultAction)
					.disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
					.accessibilityIdentifier("namePrompt.save")
			}
		}
		.padding(Spacing.s4)
		.containerIdentifier("popover.name")
	}

	private func save() {
		let trimmed = name.trimmingCharacters(in: .whitespaces)
		guard !trimmed.isEmpty else { return }
		Task {
			if await commit(trimmed) { dismiss() }
		}
	}
}

/// R-BUD-5's explicit confirmation: deleting an envelope with history moves
/// that history to another envelope, which Actual can't take back.
struct DeleteMovingHistorySheet: View {
	let screen: BudgetScreenModel
	let envelope: EnvelopeRow
	let transactions: UInt64
	let budgetedMonths: UInt64
	let taxBucket: String?
	@State private var destination: String?
	@State private var destinationBucket: (id: String, bucket: String?)?
	@State private var understood = false
	@Environment(\.dismiss) private var dismiss

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			Text("Delete \(envelope.name) and move its history?")
				.font(.text(.title, weight: .semibold))
			Text("Its \(Self.count(transactions, "transaction")) and its budget in \(Self.count(budgetedMonths, "month")) move to the envelope you choose. Actual can't undo this.")
				.fixedSize(horizontal: false, vertical: true)
			GroupedRows {
				LabeledRow("Move to") {
					Picker("Move to", selection: $destination) {
						Text("Choose an envelope").tag(String?.none)
						ForEach(screen.spendingEnvelopes.filter { $0.id != envelope.id }, id: \.id) { row in
							Text(row.name).tag(String?.some(row.id))
						}
					}
					.labelsHidden()
					.frame(width: 240)
					.accessibilityIdentifier("deleteMoving.destination")
				}
			}
			if let warning = taxWarning {
				Label(warning, systemImage: "exclamationmark.triangle")
					.foregroundStyle(.secondary)
					.fixedSize(horizontal: false, vertical: true)
					.accessibilityIdentifier("deleteMoving.taxWarning")
			}
			Toggle("Delete \(envelope.name) for good", isOn: $understood)
				.toggleStyle(.checkbox)
				.accessibilityIdentifier("deleteMoving.understood")
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				Button("Delete and move history", role: .destructive) {
					guard let destination else { return }
					Task {
						if await screen.delete(envelope.id, movingHistoryTo: destination) { dismiss() }
					}
				}
				.disabled(destination == nil || !understood)
				.accessibilityIdentifier("deleteMoving.delete")
			}
		}
		.padding(Spacing.s5)
		.frame(width: 500)
		.containerIdentifier("sheet.deleteMoving")
		.task(id: destination) {
			guard let destination else { return }
			destinationBucket = (destination, await screen.taxBucket(of: destination))
		}
	}

	private var taxWarning: String? {
		guard let destination, let loaded = destinationBucket, loaded.id == destination,
			let target = screen.spendingEnvelopes.first(where: { $0.id == destination })
		else { return nil }
		return TaxBuckets.mergeWarning(source: envelope.name, sourceBucket: taxBucket, target: target.name, targetBucket: loaded.bucket)
	}

	static func count(_ value: UInt64, _ noun: String) -> String {
		"\(value) \(noun)\(value == 1 ? "" : "s")"
	}
}

/// What dragging carries: an envelope or a group, by id.
enum BudgetDrag {
	static func envelope(_ id: String) -> String { "envelope:\(id)" }
	static func group(_ id: String) -> String { "group:\(id)" }

	static func parse(_ item: String) -> (isGroup: Bool, id: String)? {
		if item.hasPrefix("envelope:") { return (false, String(item.dropFirst(9))) }
		if item.hasPrefix("group:") { return (true, String(item.dropFirst(6))) }
		return nil
	}
}
