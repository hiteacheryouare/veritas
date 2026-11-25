import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Statements (R-REP-7, design/mockups/statement.html): one statement at a
/// time, shown as the page it prints as.
struct StatementsScreen: View {
	@Bindable var workspace: WorkspaceModel
	@State private var model: StatementsModel
	@State private var isChoosingPeriod = false
	@FocusState private var hasKeyFocus: Bool

	init(workspace: WorkspaceModel, model: StatementsModel? = nil) {
		self.workspace = workspace
		_model = State(initialValue: model ?? StatementsModel(workspace: workspace))
	}

	var body: some View {
		ScrollView {
			Group {
				if let document = model.document {
					StatementPaper(document: document, format: model.format)
				} else {
					ScreenState(kind: .loading).frame(minHeight: 200)
				}
			}
			.motion(.fast, value: model.document == nil)
			.padding(.top, Spacing.s2)
			.padding(.horizontal, Spacing.s5)
			.padding(.bottom, Spacing.s6)
			.frame(maxWidth: .infinity)
		}
		.background(.surfaceWindow)
		.focusable()
		.focusEffectDisabled()
		.focused($hasKeyFocus)
		.defaultFocus($hasKeyFocus, true)
		.focusedSceneValue(\.budgetMonths, BudgetMonthActions(
			canGoBack: true,
			canGoForward: model.canGoForward,
			previous: { Task { await model.previous() } },
			next: { Task { await model.next() } }
		))
		.toolbar {
			ToolbarItem(placement: .navigation) {
				PeriodStepper(canGoForward: model.canGoForward, previous: { Task { await model.previous() } }, next: { Task { await model.next() } }, id: "statements")
			}
			ToolbarItem(placement: .principal) {
				SpanPicker(selection: model.kind, choices: StatementKind.allCases, title: \.title, id: "statements.kind", label: "Statement") { kind in
					Task { await model.show(kind) }
				}
			}
			ToolbarItem(placement: .primaryAction) {
				Button { isChoosingPeriod = true } label: {
					HStack(spacing: Spacing.s1) {
						Text(model.periodLabel).monospacedDigit()
						Image(systemName: "chevron.up.chevron.down").imageScale(.small)
					}
				}
				.accessibilityIdentifier("statements.period")
				.popover(isPresented: $isChoosingPeriod, arrowEdge: .bottom) {
					PeriodChooser(model: model)
				}
			}
			ToolbarItem(placement: .primaryAction) {
				Menu {
					Button("\(model.kind.documentTitle)…") {
						guard let document = model.document else { return }
						Task { await StatementExport.run(document, period: model.periodFileName, format: model.format, workspace: workspace) }
					}
					.accessibilityIdentifier("statements.export.current")
					Button("All four statements…") {
						Task { await StatementExport.runSet(model, workspace: workspace) }
					}
					.accessibilityIdentifier("statements.export.all")
				} label: {
					Label("Export PDF", systemImage: "square.and.arrow.up")
						.labelStyle(.titleAndIcon)
				}
				.disabled(model.document == nil)
				.accessibilityIdentifier("statements.export")
			}
		}
		.task(id: workspace.changeCount) { await model.load() }
		.containerIdentifier("screen.statements")
	}
}

/// The balance sheet's date, or the other statements' months.
private struct PeriodChooser: View {
	let model: StatementsModel
	@State private var date: Date
	@State private var isCustom: Bool
	@Environment(\.dismiss) private var dismiss

	init(model: StatementsModel) {
		self.model = model
		_date = State(initialValue: Self.date(model.asOf) ?? .now)
		_isCustom = State(initialValue: model.span == .custom)
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s3) {
			if model.kind.isDated {
				DatePicker("As of", selection: $date, in: ...(Self.date(model.today) ?? .now), displayedComponents: .date)
					.datePickerStyle(.graphical)
					.labelsHidden()
					.accessibilityIdentifier("statements.asOf")
					.onChange(of: date) { _, chosen in
						Task { await model.setAsOf(WorkspaceModel.today(chosen)) }
					}
					.padding(Spacing.s3)
			} else {
				SpanPicker(selection: isCustom ? .custom : model.span, choices: SankeySpan.allCases, title: \.title, id: "statements.range") { span in
					if span == .custom {
						isCustom = true
					} else {
						Task { await model.choose(span) }
						dismiss()
					}
				}
				.padding([.top, .horizontal], Spacing.s4)
				if isCustom {
					MonthRangePicker(period: model.period, thisMonth: model.thisMonth) { first, last in
						Task { await model.setCustom(first: first, last: last) }
					}
				} else {
					Spacer().frame(height: Spacing.s1)
				}
			}
		}
	}

	static func date(_ text: String) -> Date? {
		CivilDate.date(text)
	}
}
