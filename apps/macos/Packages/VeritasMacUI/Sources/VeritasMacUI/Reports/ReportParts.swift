import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// A report figure in its tone. Overspent carries its icon, so red is never
/// the only signal.
struct ReportAmount: View {
	let amount: Int64
	var tone: VeritasCore.MoneyTone = .neutral
	let format: MoneyFormat
	var weight: Font.Weight = .medium
	var size: TextSize = .body

	var body: some View {
		HStack(spacing: 4) {
			if tone == .overspent {
				Image(systemName: "exclamationmark.triangle")
					.font(.system(size: size == .body ? 10 : size.rawValue * 0.6, weight: .semibold))
					.accessibilityHidden(true)
			}
			Text(format.amount(amount))
				.font(.figures(size, weight: weight))
				.monospacedDigit()
				.lineLimit(1)
				.fixedSize()
		}
		.foregroundStyle(color)
	}

	private var color: Color {
		switch tone {
		case .neutral: .textPrimary
		default: Color(tone.design.text)
		}
	}
}

/// Previous and next period, as on the Budget screen (⌘← ⌘→).
struct PeriodStepper: View {
	let canGoForward: Bool
	let previous: () -> Void
	let next: () -> Void
	let id: String

	var body: some View {
		ControlGroup {
			Button(action: previous) {
				Label("Previous", systemImage: "chevron.left")
			}
			.accessibilityIdentifier("\(id).previous")
			Button(action: next) {
				Label("Next", systemImage: "chevron.right")
			}
			.disabled(!canGoForward)
			.accessibilityIdentifier("\(id).next")
		}
		.help("Previous or next period (⌘← ⌘→)")
	}
}

/// The custom range: a first and last month, from the five years before this one.
struct MonthRangePicker: View {
	let period: ReportPeriod
	let thisMonth: Month
	let apply: (Month, Month) -> Void
	@State private var first: Month
	@State private var last: Month
	@Environment(\.dismiss) private var dismiss

	init(period: ReportPeriod, thisMonth: Month, apply: @escaping (Month, Month) -> Void) {
		self.period = period
		self.thisMonth = thisMonth
		self.apply = apply
		_first = State(initialValue: period.first)
		_last = State(initialValue: period.last)
	}

	private var choices: [Month] {
		ReportPeriod(first: Month(year: thisMonth.year - 5, month: 1), last: thisMonth).months.reversed()
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s3) {
			Picker("From", selection: $first) {
				ForEach(choices, id: \.self) { Text(ReportPeriod.monthYear($0)).tag($0) }
			}
			.accessibilityIdentifier("range.from")
			Picker("To", selection: $last) {
				ForEach(choices, id: \.self) { Text(ReportPeriod.monthYear($0)).tag($0) }
			}
			.accessibilityIdentifier("range.to")
			HStack {
				Spacer()
				Button("Show range") {
					apply(first, last)
					dismiss()
				}
				.keyboardShortcut(.defaultAction)
				.accessibilityIdentifier("range.apply")
			}
		}
		.padding(Spacing.s4)
		.frame(width: 300)
	}
}

/// The rows behind a number: date, payee, category, account and amount (ST-1.6).
struct DrilldownSheet: View {
	let model: DrilldownModel
	@Environment(\.dismiss) private var dismiss

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s3) {
			Text(model.title)
				.font(.text(.title, weight: .semibold))
			if let caption = model.caption {
				Text(caption)
					.foregroundStyle(.textSecondary)
			}
			Group {
				if let rows = model.transactions {
					if rows.isEmpty {
						Text("No transactions")
							.foregroundStyle(.textSecondary)
							.frame(maxWidth: .infinity, minHeight: 120)
					} else {
						Table(rows) {
							TableColumn("Date") { Text(Self.date($0.date)).monospacedDigit() }
								.width(min: 80, ideal: 90)
							TableColumn("Payee") { Text($0.payee ?? "") }
							TableColumn("Category") { Text($0.category ?? "") }
							TableColumn("Account") { Text($0.accountName) }
								.width(min: 80, ideal: 100)
							TableColumn("Amount") { row in
								Text(model.format.amount(row.amount))
									.font(.figures(.body))
									.frame(maxWidth: .infinity, alignment: .trailing)
							}
							.width(min: 90, ideal: 110)
						}
						.frame(minHeight: 260)
						.accessibilityIdentifier("drilldown.table")
					}
				} else {
					ProgressView()
						.frame(maxWidth: .infinity, minHeight: 120)
				}
			}
			HStack {
				if let rows = model.transactions, !rows.isEmpty {
					Text(rows.count == 1 ? "1 transaction" : "\(rows.count.formatted()) transactions")
						.foregroundStyle(.textSecondary)
						.accessibilityIdentifier("drilldown.count")
				}
				Spacer()
				Button("Done") { dismiss() }
					.keyboardShortcut(.defaultAction)
			}
		}
		.padding(Spacing.s5)
		.frame(width: 680)
		.task { await model.load() }
		.containerIdentifier("sheet.drilldown")
	}

	static func date(_ text: String) -> String {
		let parts = text.split(separator: "-").compactMap { Int($0) }
		guard parts.count == 3, let date = Calendar.current.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2])) else { return text }
		return date.formatted(.dateTime.month(.abbreviated).day().year())
	}
}
