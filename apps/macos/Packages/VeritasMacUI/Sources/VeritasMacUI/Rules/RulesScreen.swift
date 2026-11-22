import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Every rule as a sentence, grouped by the stage Actual runs it in
/// (R-RULE-2, R-RULE-3). Editable rules open the sentence editor; a bill's
/// rule opens its bill; the rest are read-only.
struct RulesScreen: View {
	static let stages: [RuleStage] = [.pre, .default, .post]

	let workspace: WorkspaceModel
	@State private var model: RulesModel

	init(workspace: WorkspaceModel, model: RulesModel? = nil) {
		self.workspace = workspace
		_model = State(initialValue: model ?? RulesModel(workspace: workspace))
	}

	var body: some View {
		ScrollView {
			VStack(alignment: .leading, spacing: Spacing.s6) {
				ForEach(Self.stages, id: \.self) { stage in
					let rules = model.rules.filter { $0.stage == stage }
					if !rules.isEmpty {
						VStack(alignment: .leading, spacing: Spacing.s2) {
							SectionTitle(RuleWords.stage(stage))
							GroupedRows {
								ForEach(rules, id: \.id) { rule in
									RuleRow(model: model, rule: rule)
									if rule.id != rules.last?.id {
										Divider().padding(.leading, Spacing.s4)
									}
								}
							}
						}
						.containerIdentifier("rules.stage.\(RuleWords.stage(stage).lowercased())")
					}
				}
			}
			.padding(Spacing.s5)
			.frame(maxWidth: .infinity, alignment: .leading)
		}
		.overlay { if !model.hasLoaded { ScreenState(kind: .loading) } }
		.background(.surfaceWindow)
		.toolbar {
			ToolbarItem(placement: .primaryAction) {
				Button {
					model.startNew()
				} label: {
					Label("New rule", systemImage: "plus")
				}
				.labelStyle(.titleAndIcon)
				.disabled(model.isReadOnly)
				.accessibilityIdentifier("rules.new")
			}
		}
		.sheet(isPresented: Binding(get: { model.editing != nil }, set: { if !$0 { model.cancelEditing() } })) {
			RuleEditorSheet(model: model)
		}
		.sheet(isPresented: Binding(get: { model.applying != nil && model.editing == nil }, set: { if !$0 { model.applying = nil } })) {
			RuleApplySheet(model: model)
		}
		.task(id: workspace.changeCount) { await model.load() }
		.containerIdentifier("screen.rules")
	}
}

private struct RuleRow: View {
	let model: RulesModel
	let rule: RuleSentence
	@State private var showsReasons = false

	private var schedule: BillDate? {
		if case .bill(let scheduleId) = rule.access { return model.schedules[scheduleId] }
		return nil
	}

	var body: some View {
		HStack(alignment: .top, spacing: Spacing.s4) {
			RuleSentenceText(rule: rule, names: model.names, format: model.format, schedule: schedule)
				.frame(maxWidth: .infinity, alignment: .leading)
			VStack(alignment: .trailing, spacing: Spacing.s1) {
				trailing
				if !rule.runsInVeritas {
					Text("Only Actual runs this rule")
						.font(.text(.caption))
						.foregroundStyle(.textSecondary)
						.accessibilityIdentifier("rule.\(rule.id).actualOnly")
				}
			}
		}
		.padding(.horizontal, Spacing.s4)
		.padding(.vertical, Spacing.s3)
		.frame(minHeight: Size.row)
		.contentShape(.rect)
		.onTapGesture(count: 2) { open() }
		.contextMenu {
			if case .editable = rule.access { editableMenu }
		}
		.accessibilityElement(children: .contain)
		.accessibilityIdentifier("rule.\(rule.id)")
	}

	@ViewBuilder private var trailing: some View {
		switch rule.access {
		case .editable:
			Menu {
				editableMenu
			} label: {
				Image(systemName: "ellipsis")
					.accessibilityLabel("Rule actions")
			}
			.menuStyle(.borderlessButton)
			.menuIndicator(.hidden)
			.fixedSize()
			.accessibilityIdentifier("rule.\(rule.id).menu")
		case .bill(let scheduleId):
			Button("Open bill") { model.openBill(scheduleId) }
				.buttonStyle(.link)
				.accessibilityIdentifier("rule.\(rule.id).openBill")
		case .editInActual(let reasons):
			Button {
				showsReasons = true
			} label: {
				Label("Edit in Actual", systemImage: "exclamationmark.triangle")
			}
			.buttonStyle(.borderless)
			.foregroundStyle(.moneyLow)
			.accessibilityIdentifier("rule.\(rule.id).editInActual")
			.popover(isPresented: $showsReasons, arrowEdge: .bottom) {
				Text("Only Actual can edit this rule. It has \(ListFormatter.localizedString(byJoining: reasons)).")
					.font(.text(.body))
					.fixedSize(horizontal: false, vertical: true)
					.frame(width: 260, alignment: .leading)
					.padding(Spacing.s3)
					.accessibilityIdentifier("rule.\(rule.id).reasons")
			}
		}
	}

	@ViewBuilder private var editableMenu: some View {
		Button("Edit…") { model.edit(rule) }
			.disabled(model.isReadOnly)
		Button("Apply to Existing…") { Task { await model.preview(.saved(ruleId: rule.id)) } }
			.disabled(model.isReadOnly)
		Divider()
		Button("Delete", role: .destructive) { Task { await model.delete(rule.id) } }
			.disabled(model.isReadOnly)
	}

	private func open() {
		switch rule.access {
		case .editable: if !model.isReadOnly { model.edit(rule) }
		case .bill(let scheduleId): model.openBill(scheduleId)
		case .editInActual: showsReasons = true
		}
	}
}

/// `When imported payee contains "AMZN"`, then each action on its own
/// indented line, split lines last.
struct RuleSentenceText: View {
	let rule: RuleSentence
	let names: BudgetNames
	let format: MoneyFormat
	var schedule: BillDate?

	var body: some View {
		VStack(alignment: .leading, spacing: 2) {
			ForEach(Array(rule.conditions.enumerated()), id: \.offset) { index, condition in
				Text("\(lead(index)) \(RuleWords.condition(condition, names: names, format: format, schedule: schedule))")
					.foregroundStyle(.textPrimary)
			}
			ForEach(Array(actionLines.enumerated()), id: \.offset) { _, line in
				HStack(alignment: .firstTextBaseline, spacing: Spacing.s2) {
					Text("→").foregroundStyle(.textTertiary).accessibilityHidden(true)
					Text(line)
				}
				.foregroundStyle(.textSecondary)
				.padding(.leading, Spacing.s4)
			}
		}
		.font(.text(.body))
		.textSelection(.enabled)
	}

	private func lead(_ index: Int) -> String {
		if index == 0 { return "When" }
		return rule.conditionsOp == .or ? "or" : "and"
	}

	private var actionLines: [String] {
		let words = { (action: RuleAction) in RuleWords.action(action, names: names, format: format) }
		let plain = rule.actions.filter { $0.splitIndex == 0 }.map(words)
		let lines = Array(Set(rule.actions.map(\.splitIndex).filter { $0 > 0 })).sorted()
		let splits = lines.map { line in
			let parts = rule.actions.filter { $0.splitIndex == line }
			let ordered = parts.filter { $0.op == .setSplitAmount } + parts.filter { $0.op != .setSplitAmount }
			return "split \(line): " + ordered.map(words).joined(separator: ", ")
		}
		return plain + splits
	}
}
