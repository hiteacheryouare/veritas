import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// The upcoming tray and every bill, or the next 60 days by month (R-BILL-1
/// to R-BILL-5). In Upcoming, ↑ and ↓ move between bills and Return opens
/// Paid (design/interaction.md).
struct BillsScreen: View {
	@Bindable var workspace: WorkspaceModel
	@State private var model: BillsModel
	@State private var showsTimeline: Bool
	@State private var paying: String?
	@State private var selection: String?
	@FocusState private var hasKeyFocus: Bool

	init(workspace: WorkspaceModel, model: BillsModel? = nil, showsTimeline: Bool = false, paying: String? = nil) {
		self.workspace = workspace
		_model = State(initialValue: model ?? BillsModel(workspace: workspace))
		_showsTimeline = State(initialValue: showsTimeline)
		_paying = State(initialValue: paying)
	}

	var body: some View {
		ScrollView {
			VStack(alignment: .leading, spacing: Spacing.s6) {
				if showsTimeline {
					BillTimeline(model: model)
				} else {
					upcoming
					BillList(title: "All bills", bills: model.activeBills, model: model)
					if !model.stoppedBills.isEmpty {
						BillList(title: "Paused and ended", bills: model.stoppedBills, model: model)
					}
				}
			}
			.padding(Spacing.s5)
			.frame(maxWidth: .infinity, alignment: .leading)
		}
		.background(.surfaceWindow)
		.focusable()
		.focusEffectDisabled()
		.focused($hasKeyFocus)
		.onKeyPress(keys: [.upArrow, .downArrow]) { press in
			guard paying == nil, !showsTimeline, press.modifiers.isDisjoint(with: BudgetKeys.chordKeys) else { return .ignored }
			moveSelection(by: press.key == .upArrow ? -1 : 1)
			return .handled
		}
		.onKeyPress(.return) {
			guard paying == nil, !showsTimeline, !model.isReadOnly, let selection else { return .ignored }
			paying = selection
			return .handled
		}
		.toolbar {
			ToolbarItemGroup(placement: .primaryAction) {
				Picker("View", selection: $showsTimeline) {
					Text("List").tag(false)
					Text("Timeline").tag(true)
				}
				.pickerStyle(.segmented)
				.fixedSize()
				.accessibilityIdentifier("bills.view")
				Button {
					model.startNew()
				} label: {
					Label("New bill", systemImage: "plus")
				}
				.labelStyle(.titleAndIcon)
				.disabled(model.isReadOnly)
				.accessibilityIdentifier("bills.new")
			}
		}
		.sheet(isPresented: Binding(get: { model.editing != nil }, set: { if !$0 { model.cancelEditing() } })) {
			if let editing = model.editing {
				BillEditor(model: model, editing: editing)
			}
		}
		.task(id: workspace.changeCount) { await model.load() }
		.onAppear { Task { @MainActor in hasKeyFocus = true } }
		.containerIdentifier("screen.bills")
	}

	private var upcoming: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			SectionTitle("Upcoming")
			if model.tray.isEmpty {
				Text("Nothing due in the next 7 days.")
					.font(.text(.body))
					.foregroundStyle(.textSecondary)
					.padding(.leading, Spacing.s1)
			} else {
				GroupedRows {
					ForEach(model.tray, id: \.self) { item in
						UpcomingRow(
							item: item,
							model: model,
							isSelected: selection == item.billId && hasKeyFocus,
							isPaying: paying == item.billId,
							openPay: {
								selection = item.billId
								paying = item.billId
							},
							closePay: {
								paying = nil
								hasKeyFocus = true
							}
						)
						if item != model.tray.last {
							Divider().padding(.leading, Spacing.s4)
						}
					}
				}
			}
		}
	}

	private func moveSelection(by step: Int) {
		let ids = model.tray.map(\.billId)
		guard !ids.isEmpty else { return }
		guard let current = selection.flatMap({ ids.firstIndex(of: $0) }) else {
			selection = step > 0 ? ids.first : ids.last
			return
		}
		selection = ids[min(max(current + step, 0), ids.count - 1)]
	}
}

/// "about" in the text face before an estimate's figure, and income in its
/// own color with its icon.
struct BillAmountLabel: View {
	let amount: BillAmount
	let isIncome: Bool
	let format: MoneyFormat
	var weight: Font.Weight = .medium

	var body: some View {
		let words = BillWords.amount(amount, format: format)
		HStack(spacing: 3) {
			if isIncome, let symbol = MoneyTone.income.symbol {
				Image(systemName: symbol)
					.imageScale(.small)
					.foregroundStyle(.moneyIncome)
			}
			if let qualifier = words.qualifier {
				Text(qualifier)
					.font(.text(.body))
					.foregroundStyle(.textSecondary)
			}
			Text(words.figures)
				.font(.figures(.body, weight: weight))
				.foregroundStyle(isIncome ? AnyShapeStyle(.moneyIncome) : AnyShapeStyle(.textPrimary))
		}
		.lineLimit(1)
	}

	static func words(_ amount: BillAmount, isIncome: Bool, format: MoneyFormat) -> String {
		let words = BillWords.amount(amount, format: format)
		return [isIncome ? "income" : nil, words.qualifier, words.figures].compactMap(\.self).joined(separator: " ")
	}
}
