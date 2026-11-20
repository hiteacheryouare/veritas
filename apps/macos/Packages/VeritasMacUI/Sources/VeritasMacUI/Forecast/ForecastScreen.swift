import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Forecast (R-FC-1 to R-FC-4, design/mockups/forecast.html): net worth at
/// the chosen horizon with its band and the core's band label, the chart,
/// the compare against up to 3 what-ifs, and below them goal ETAs, next
/// month's suggestions and the lists the forecast is built from.
struct ForecastScreen: View {
	@Bindable var workspace: WorkspaceModel
	@State private var model: ForecastModel
	@State private var whatIfs: WhatIfsModel
	@State private var isAddingWhatIf = false
	@FocusState private var hasKeyFocus: Bool

	init(workspace: WorkspaceModel, model: ForecastModel? = nil, whatIfs: WhatIfsModel? = nil) {
		self.workspace = workspace
		_model = State(initialValue: model ?? ForecastModel(workspace: workspace))
		_whatIfs = State(initialValue: whatIfs ?? WhatIfsModel(workspace: workspace))
	}

	var body: some View {
		ScrollView {
			VStack(alignment: .leading, spacing: Spacing.s5) {
				if let forecast = model.forecast {
					VStack(alignment: .leading, spacing: Spacing.s4) {
						ForecastHero(model: model, forecast: forecast)
						NetWorthForecastChart(model: model, forecast: forecast)
							.padding(Spacing.s4)
							.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
							.overlay { RoundedRectangle(cornerRadius: Radius.section).strokeBorder(Color.edgeGrouped) }
					}
					CompareSection(model: model) { isAddingWhatIf = true }
					AccountsSection(model: model, forecast: forecast)
					if !forecast.baseline.goalEtas.isEmpty {
						GoalEtaSection(model: model)
					}
					if let suggestions = model.suggestions {
						SuggestionsSection(model: model, suggestions: suggestions)
					}
					ForecastLists(model: model, forecast: forecast)
				} else {
					ProgressView().frame(maxWidth: .infinity, minHeight: 200)
				}
			}
			.padding(.top, Spacing.s2)
			.padding(.horizontal, Spacing.s5)
			.padding(.bottom, Spacing.s6)
			.frame(maxWidth: 980, alignment: .leading)
			.frame(maxWidth: .infinity, alignment: .leading)
		}
		.background(.surfaceWindow)
		.focusable()
		.focusEffectDisabled()
		.focused($hasKeyFocus)
		.defaultFocus($hasKeyFocus, true)
		.toolbar {
			ToolbarItem(placement: .primaryAction) {
				Picker("Horizon", selection: $model.horizon) {
					ForEach(ForecastModel.horizons, id: \.self) { Text("\($0) months").tag($0) }
				}
				.pickerStyle(.segmented)
				.labelsHidden()
				.fixedSize()
				.accessibilityIdentifier("forecast.horizon")
			}
		}
		.sheet(isPresented: $isAddingWhatIf) {
			NewWhatIfSheet(model: whatIfs)
		}
		.task(id: ReloadKey(changeCount: workspace.changeCount, compared: workspace.comparedScenarios)) {
			await model.load()
		}
		.task { await whatIfs.load() }
		.containerIdentifier("screen.forecast")
	}
}

private struct ReloadKey: Hashable {
	let changeCount: Int
	let compared: [String]
}

private struct ForecastHero: View {
	let model: ForecastModel
	let forecast: Forecast

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s1) {
			if let point = model.selected {
				HStack(alignment: .firstTextBaseline, spacing: Spacing.s3) {
					Text(model.format.wholeCurrency(point.netWorth, on: .register))
						.font(.figures(.display, weight: .bold))
					Text("net worth in \(ReportPeriod.monthYear(point.month))")
						.font(.text(.heading, weight: .semibold))
						.foregroundStyle(.textSecondary)
				}
				.accessibilityElement(children: .combine)
				.accessibilityIdentifier("forecast.hero")
				HStack(alignment: .firstTextBaseline, spacing: Spacing.s2) {
					if let band = ForecastWords(format: model.format).band(point) {
						Text(band).font(.figures(.body, weight: .semibold))
					}
					Text(forecast.bandLabel).foregroundStyle(.textSecondary)
				}
				.accessibilityElement(children: .combine)
				.accessibilityIdentifier("forecast.band")
			}
			ForEach([forecast.notForecastNote, forecast.assetPurchaseNote].compactMap { $0 }, id: \.self) { note in
				Text(note).foregroundStyle(.textSecondary)
			}
		}
	}
}

// MARK: Compare

private struct CompareSection: View {
	let model: ForecastModel
	let addWhatIf: () -> Void

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			HStack(alignment: .firstTextBaseline) {
				SectionTitle("Compare at \(model.horizon) months")
				Spacer()
				Button(action: addWhatIf) {
					Label("Add what-if", systemImage: "plus")
				}
				.disabled(model.isReadOnly)
				.accessibilityIdentifier("forecast.addWhatIf")
			}
			GroupedRows {
				ForEach(Array(model.projections.enumerated()), id: \.offset) { index, projection in
					if index > 0 { Divider().overlay(Color.separator).padding(.leading, Spacing.s4) }
					CompareRow(model: model, projection: projection, series: index)
				}
				ForEach(model.notCompared, id: \.id) { scenario in
					Divider().overlay(Color.separator).padding(.leading, Spacing.s4)
					HStack(spacing: Spacing.s3) {
						Color.clear.frame(width: 22, height: 6)
						Text(scenario.name).foregroundStyle(.textSecondary)
						Spacer()
						Button("Compare") { model.setCompared(scenario.id, true) }
							.controlSize(.small)
							.disabled(!model.canCompareMore)
							.help(model.canCompareMore ? "" : "Up to \(ForecastModel.maxCompared) what-ifs at once")
							.accessibilityIdentifier("forecast.compare.add.\(scenario.id)")
					}
					.padding(.horizontal, Spacing.s4)
					.frame(minHeight: 44)
				}
			}
		}
	}
}

private struct CompareRow: View {
	let model: ForecastModel
	let projection: ForecastProjection
	let series: Int

	private var name: String { projection.scenarioId == nil ? "Baseline" : projection.name }

	private var inThePast: String? {
		switch projection.eventsInThePast.count {
		case 0: nil
		case 1: "1 event in the past, left out"
		case let count: "\(count) events in the past, left out"
		}
	}

	var body: some View {
		let point = model.point(projection)
		HStack(spacing: Spacing.s3) {
			SeriesSwatch(series: series)
			VStack(alignment: .leading, spacing: 2) {
				Text(name).font(.text(.body, weight: .medium))
				if let inThePast {
					Text(inThePast).font(.text(.caption)).foregroundStyle(.textSecondary)
				}
			}
			Spacer(minLength: Spacing.s4)
			if let point {
				Text(model.format.wholeCurrency(point.netWorth, on: .register))
					.font(.figures(.body, weight: .semibold))
			}
			if let id = projection.scenarioId {
				Button("Remove") { model.setCompared(id, false) }
					.controlSize(.small)
					.accessibilityIdentifier("forecast.compare.remove.\(id)")
			}
		}
		.padding(.horizontal, Spacing.s4)
		.frame(minHeight: 48)
		.accessibilityElement(children: .combine)
		.accessibilityIdentifier("forecast.compare.\(projection.scenarioId ?? "baseline")")
	}
}

// MARK: Accounts

private struct AccountsSection: View {
	let model: ForecastModel
	let forecast: Forecast

	var body: some View {
		if let point = model.selected {
			VStack(alignment: .leading, spacing: Spacing.s2) {
				SectionTitle("Accounts")
				Grid(alignment: .trailing, horizontalSpacing: Spacing.s5, verticalSpacing: 0) {
					GridRow {
						Text("").gridColumnAlignment(.leading)
						Text("Today")
						Text(ReportPeriod.monthYear(point.month))
					}
					.font(.text(.caption, weight: .medium))
					.foregroundStyle(.textSecondary)
					.frame(minHeight: 28)
					ForEach(forecast.accountsToday, id: \.accountId) { account in
						Divider().overlay(Color.separator).gridCellUnsizedAxes(.horizontal)
						GridRow {
							Text(account.name).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
							ReportAmount(amount: account.balance, format: model.format)
							if let later = point.accounts.first(where: { $0.accountId == account.accountId }) {
								ReportAmount(amount: later.balance, format: model.format)
							} else {
								Text("")
							}
						}
						.frame(minHeight: 34)
					}
				}
				.padding(.horizontal, Spacing.s4)
				.padding(.vertical, Spacing.s1)
				.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
				.overlay { RoundedRectangle(cornerRadius: Radius.section).strokeBorder(Color.edgeGrouped) }
				.containerIdentifier("forecast.accounts")
			}
		}
	}
}

// MARK: Goals

private struct GoalEtaSection: View {
	let model: ForecastModel

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			SectionTitle("Goals")
			GroupedRows {
				ForEach(Array(model.goalRows.enumerated()), id: \.element.id) { index, row in
					if index > 0 { Divider().overlay(Color.separator).padding(.leading, Spacing.s4) }
					VStack(alignment: .leading, spacing: Spacing.s1) {
						HStack(alignment: .firstTextBaseline) {
							Text(row.name).font(.text(.body, weight: .medium))
							Spacer(minLength: Spacing.s4)
							if row.onTrack == false {
								Image(systemName: VeritasDesign.MoneyTone.low.symbol ?? "arrow.down.circle")
									.imageScale(.small)
									.foregroundStyle(.moneyLow)
									.accessibilityLabel("After the target date")
							}
							Text(row.text)
								.foregroundStyle(row.onTrack == false ? Color.moneyLow : Color.textSecondary)
						}
						ForEach(row.whatIfs, id: \.index) { whatIf in
							HStack(spacing: Spacing.s2) {
								Spacer()
								SeriesSwatch(series: whatIf.index + 1)
								Text("\(whatIf.name): \(whatIf.text)").foregroundStyle(.textSecondary)
							}
							.font(.text(.caption))
						}
					}
					.padding(.horizontal, Spacing.s4)
					.padding(.vertical, Spacing.s2)
					.frame(minHeight: 40)
					.accessibilityElement(children: .combine)
					.accessibilityIdentifier("forecast.goal.\(row.id)")
				}
			}
		}
	}
}

// MARK: Suggestions

private struct SuggestionsSection: View {
	let model: ForecastModel
	let suggestions: SuggestionList
	@State private var justApplied = false

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			HStack(alignment: .firstTextBaseline) {
				SectionTitle("Suggested budget for \(ReportPeriod.monthYear(suggestions.month))")
				Spacer()
				if justApplied {
					Label("Applied", systemImage: "checkmark")
						.foregroundStyle(.textSecondary)
						.accessibilityIdentifier("forecast.suggestions.applied")
				}
				Button("Apply suggestions") {
					Task {
						if await model.applySuggestions() {
							justApplied = true
							try? await Task.sleep(for: .seconds(4))
							justApplied = false
						}
					}
				}
				.disabled(!model.canApplySuggestions)
				.accessibilityIdentifier("forecast.applySuggestions")
			}
			if let notice = suggestions.notice {
				ProblemNote(text: notice)
					.accessibilityIdentifier("forecast.suggestions.notice")
			}
			GroupedRows {
				ForEach(Array(suggestions.items.enumerated()), id: \.element.categoryId) { index, item in
					if index > 0 { Divider().overlay(Color.separator).padding(.leading, Spacing.s4) }
					Text(item.reason)
						.foregroundStyle(item.amount == nil ? Color.textSecondary : Color.textPrimary)
						.frame(maxWidth: .infinity, minHeight: 36, alignment: .leading)
						.padding(.horizontal, Spacing.s4)
						.accessibilityIdentifier("forecast.suggestion.\(item.categoryId)")
				}
				Divider().overlay(Color.separator)
				LabeledRow("Expected income") {
					ReportAmount(amount: suggestions.expectedIncome, format: model.format, weight: .semibold)
				}
			}
			.containerIdentifier("forecast.suggestions")
		}
	}
}

// MARK: Lists

private struct ForecastLists: View {
	let model: ForecastModel
	let forecast: Forecast
	@State private var showsFlows = false
	@State private var showsBaselines = false

	private var words: ForecastWords { ForecastWords(format: model.format) }

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s5) {
			if !forecast.shortfalls.isEmpty {
				ListSection(title: "Payments short of interest and escrow", id: "forecast.shortfalls") {
					ForEach(Array(forecast.shortfalls.enumerated()), id: \.offset) { index, item in
						if index > 0 { Divider().overlay(Color.separator) }
						ProblemNote(text: words.shortfall(item))
							.frame(maxWidth: .infinity, minHeight: 36, alignment: .leading)
							.padding(.horizontal, Spacing.s4)
					}
				}
			}
			if !forecast.unforecast.isEmpty {
				ListSection(title: "Not forecast", id: "forecast.unforecast") {
					ForEach(Array(forecast.unforecast.enumerated()), id: \.offset) { index, item in
						if index > 0 { Divider().overlay(Color.separator) }
						Text("\(item.name): \(item.reason)")
							.frame(maxWidth: .infinity, minHeight: 36, alignment: .leading)
							.padding(.horizontal, Spacing.s4)
					}
				}
			}
			if !forecast.possibleDoubleCounts.isEmpty {
				ListSection(title: "May be counted twice", id: "forecast.doubleCounts") {
					ForEach(Array(forecast.possibleDoubleCounts.enumerated()), id: \.element.categoryId) { index, category in
						if index > 0 { Divider().overlay(Color.separator) }
						Text(category.name)
							.frame(maxWidth: .infinity, minHeight: 36, alignment: .leading)
							.padding(.horizontal, Spacing.s4)
					}
				}
			}
			if !forecast.loans.isEmpty {
				ListSection(title: "Loans", id: "forecast.loans") {
					ForEach(Array(forecast.loans.enumerated()), id: \.element.accountId) { index, loan in
						if index > 0 { Divider().overlay(Color.separator) }
						VStack(alignment: .leading, spacing: 2) {
							Text(loan.name).font(.text(.body, weight: .medium))
							ForEach(loan.notes, id: \.self) { note in
								Text(note).font(.text(.caption)).foregroundStyle(.textSecondary)
							}
						}
						.frame(maxWidth: .infinity, minHeight: 36, alignment: .leading)
						.padding(.horizontal, Spacing.s4)
						.padding(.vertical, Spacing.s2)
					}
				}
			}
			DisclosureGroup(isExpanded: $showsFlows) {
				GroupedRows {
					ForEach(Array(forecast.knownFlows.enumerated()), id: \.offset) { index, flow in
						if index > 0 { Divider().overlay(Color.separator) }
						FlowRow(flow: flow, route: words.route(flow, accounts: forecast.accountsToday), format: model.format)
					}
				}
				.padding(.top, Spacing.s2)
			} label: {
				SectionTitle("Known flows")
			}
			.accessibilityIdentifier("forecast.flows")
			DisclosureGroup(isExpanded: $showsBaselines) {
				GroupedRows {
					ForEach(Array(forecast.baselines.enumerated()), id: \.element.categoryId) { index, baseline in
						if index > 0 { Divider().overlay(Color.separator) }
						HStack(spacing: Spacing.s3) {
							Text(baseline.name).lineLimit(1)
							Spacer(minLength: Spacing.s4)
							Text(baseline.seasonal ? "\(baseline.label), seasonal" : baseline.label)
								.foregroundStyle(.textSecondary)
							ReportAmount(amount: baseline.level, format: model.format)
								.frame(minWidth: 90, alignment: .trailing)
						}
						.padding(.horizontal, Spacing.s4)
						.frame(minHeight: 34)
						.accessibilityElement(children: .combine)
					}
				}
				.padding(.top, Spacing.s2)
			} label: {
				SectionTitle("Category baselines")
			}
			.accessibilityIdentifier("forecast.baselines")
		}
	}
}

private struct ListSection<Content: View>: View {
	let title: String
	let id: String
	@ViewBuilder let content: Content

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			SectionTitle(title)
			GroupedRows { content }
				.containerIdentifier(id)
		}
	}
}

private struct FlowRow: View {
	let flow: ForecastFlow
	let route: String
	let format: MoneyFormat

	var body: some View {
		HStack(spacing: Spacing.s3) {
			Text(ForecastWords.shortDay(flow.date))
				.font(.figures(.body, weight: .regular))
				.foregroundStyle(.textSecondary)
				.frame(width: 96, alignment: .leading)
			VStack(alignment: .leading, spacing: 2) {
				Text(flow.name).lineLimit(1)
				Text(route).font(.text(.caption)).foregroundStyle(.textSecondary).lineLimit(1)
			}
			Spacer(minLength: Spacing.s4)
			if flow.dueNow {
				Text("Due now").font(.text(.caption)).foregroundStyle(.moneyLow)
			}
			if flow.certainty == .estimated {
				Text("Estimated").font(.text(.caption)).foregroundStyle(.textSecondary)
			}
			ReportAmount(amount: flow.amount, format: format)
				.frame(minWidth: 90, alignment: .trailing)
		}
		.padding(.horizontal, Spacing.s4)
		.frame(minHeight: 40)
		.accessibilityElement(children: .combine)
	}
}
