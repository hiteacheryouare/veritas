import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// The Tax summary (R-REP-8, docs/finance/tax-buckets.md), in the Net Profit
/// table's style: each bucket's total opens into its rows, and the rows whose
/// tags name more than one bucket sit under "Needs a decision".
struct TaxScreen: View {
	let workspace: WorkspaceModel
	@State private var model: TaxSummaryModel
	@FocusState private var hasKeyFocus: Bool

	init(workspace: WorkspaceModel, model: TaxSummaryModel? = nil) {
		self.workspace = workspace
		_model = State(initialValue: model ?? TaxSummaryModel(workspace: workspace))
	}

	var body: some View {
		ScrollView {
			VStack(alignment: .leading, spacing: Spacing.s4) {
				if let summary = model.summary {
					HStack(alignment: .firstTextBaseline, spacing: Spacing.s3) {
						Text("Tax summary")
							.font(.text(.title, weight: .bold))
						Text(String(summary.year))
							.font(.text(.heading, weight: .semibold))
							.foregroundStyle(.textSecondary)
					}
					.accessibilityElement(children: .combine)
					.accessibilityAddTraits(.isHeader)
					.accessibilityIdentifier("tax.title")
					if model.buckets.isEmpty, model.conflicts.isEmpty {
						Text("No transactions in a tax bucket in \(String(summary.year))")
							.foregroundStyle(.textSecondary)
							.frame(maxWidth: .infinity, minHeight: 160)
							.accessibilityIdentifier("tax.empty")
					}
					if !model.buckets.isEmpty {
						TaxBucketTable(model: model)
					}
					if !model.conflicts.isEmpty {
						VStack(alignment: .leading, spacing: Spacing.s2) {
							SectionTitle("Needs a decision")
							GroupedRows {
								ForEach(Array(model.conflicts.enumerated()), id: \.element.transactionId) { index, row in
									if index > 0 { Divider().overlay(Color.separator) }
									TaxDetailLine(row: row, amount: row.amount, format: model.format, indent: Spacing.s4)
								}
							}
							.containerIdentifier("tax.conflicts")
						}
						.padding(.top, Spacing.s2)
					}
				} else {
					ProgressView().frame(maxWidth: .infinity, minHeight: 200)
				}
			}
			.padding(.top, Spacing.s2)
			.padding(.horizontal, Spacing.s5)
			.padding(.bottom, Spacing.s6)
			.frame(maxWidth: 760, alignment: .leading)
			.frame(maxWidth: .infinity, alignment: .leading)
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
			ToolbarItemGroup(placement: .navigation) {
				PeriodStepper(canGoForward: model.canGoForward, previous: { Task { await model.previous() } }, next: { Task { await model.next() } }, id: "tax")
				Picker("Year", selection: Binding(get: { model.year }, set: { year in Task { await model.choose(year) } })) {
					ForEach(model.years, id: \.self) { Text(String($0)).tag($0) }
				}
				.pickerStyle(.menu)
				.labelsHidden()
				.fixedSize()
				.accessibilityIdentifier("tax.yearPicker")
			}
			ToolbarItem(placement: .primaryAction) {
				Menu {
					Button("Summary CSV…") { Task { await TaxExportPanel.run(.summary, for: model) } }
						.accessibilityIdentifier("tax.export.summary")
					Button("Detail CSV…") { Task { await TaxExportPanel.run(.detail, for: model) } }
						.accessibilityIdentifier("tax.export.detail")
				} label: {
					Label("Export", systemImage: "square.and.arrow.up")
				}
				.disabled(model.summary == nil)
				.accessibilityIdentifier("tax.export")
			}
		}
		.task(id: workspace.changeCount) { await model.load() }
		.containerIdentifier("screen.tax")
	}
}

private struct TaxBucketTable: View {
	let model: TaxSummaryModel

	var body: some View {
		VStack(spacing: 0) {
			ForEach(Array(model.buckets.enumerated()), id: \.element.bucket) { index, total in
				if index > 0 { Divider().overlay(Color.separator) }
				BucketRow(total: total, isExpanded: model.expanded.contains(total.bucket), format: model.format) {
					model.toggle(total.bucket)
				}
				if model.expanded.contains(total.bucket) {
					ForEach(model.rows(in: total.bucket), id: \.transactionId) { row in
						Divider().overlay(Color.separator).padding(.leading, 44)
						TaxDetailLine(row: row, amount: row.counted ?? row.amount, format: model.format, indent: 44)
					}
				}
			}
		}
		.padding(.vertical, Spacing.s1)
		.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
		.overlay { RoundedRectangle(cornerRadius: Radius.section).strokeBorder(Color.edgeGrouped) }
		.containerIdentifier("tax.buckets")
	}
}

private struct BucketRow: View {
	let total: TaxBucketTotal
	let isExpanded: Bool
	let format: MoneyFormat
	let toggle: () -> Void

	/// TAX-4: a negative total stays negative, with a neutral note under it.
	private var note: String? { total.total < 0 ? TaxBuckets.negativeNote : nil }

	var body: some View {
		let name = TaxBuckets.name(total.bucket)
		Button(action: toggle) {
			VStack(alignment: .trailing, spacing: 2) {
				HStack(spacing: 5) {
					Image(systemName: isExpanded ? "chevron.down" : "chevron.right")
						.font(.system(size: 10, weight: .semibold))
						.foregroundStyle(.textTertiary)
						.frame(width: 12)
					Text(name)
						.font(.text(.body, weight: .medium))
						.foregroundStyle(.textPrimary)
						.lineLimit(1)
					Spacer(minLength: Spacing.s4)
					ReportAmount(amount: total.total, format: format, weight: .semibold)
				}
				if let note {
					Text(note)
						.font(.text(.caption))
						.foregroundStyle(.textSecondary)
				}
			}
			.padding(.horizontal, Spacing.s4)
			.padding(.vertical, note == nil ? 0 : Spacing.s2)
			.frame(minHeight: 38)
			.contentShape(.rect)
		}
		.buttonStyle(.plain)
		.help(ReportWords.showTransactions(total.count) ?? "")
		.accessibilityLabel(["\(name), \(format.amount(total.total, on: .register))", note].compactMap { $0 }.joined(separator: ". "))
		.accessibilityHint(ReportWords.showTransactions(total.count) ?? "")
		.accessibilityValue(isExpanded ? "expanded" : "collapsed")
		.accessibilityIdentifier("tax.bucket.\(TaxBuckets.id(total.bucket))")
	}
}

/// One of the core's detail rows: date, category, and an amount on the register scale.
private struct TaxDetailLine: View {
	let row: TaxDetailRow
	let amount: Int64
	let format: MoneyFormat
	let indent: CGFloat

	var body: some View {
		HStack(spacing: Spacing.s3) {
			Text(Self.date(row.date))
				.font(.figures(.body, weight: .regular))
				.foregroundStyle(.textSecondary)
				.frame(width: 56, alignment: .leading)
			Text(row.category ?? ReportWords.uncategorized)
				.foregroundStyle(.textSecondary)
				.lineLimit(1)
			if row.fromTag {
				Image(systemName: "number")
					.imageScale(.small)
					.foregroundStyle(.textTertiary)
					.help("Tagged")
					.accessibilityLabel("Tagged")
			}
			Spacer(minLength: Spacing.s4)
			ReportAmount(amount: amount, format: format, weight: .regular)
		}
		.font(.text(.body))
		.padding(.leading, indent)
		.padding(.trailing, Spacing.s4)
		.frame(minHeight: 32)
		.accessibilityElement(children: .combine)
		.accessibilityIdentifier("tax.row.\(row.transactionId)")
	}

	/// "Mar 14": the year is the screen's.
	static func date(_ text: String) -> String {
		let parts = text.split(separator: "-").compactMap { Int($0) }
		guard parts.count == 3, let date = Calendar.current.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2])) else { return text }
		return date.formatted(.dateTime.month(.abbreviated).day())
	}
}
