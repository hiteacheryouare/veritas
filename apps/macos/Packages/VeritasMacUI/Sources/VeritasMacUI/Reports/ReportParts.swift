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
			Text(format.amount(amount, on: .register))
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

/// One line of a `PinnedTable`: its first column and one cell per column, or
/// `span`, which runs the card's full width, such as a note under a line.
struct TableLine: Identifiable {
	enum Rule {
		case light, strong
	}

	let id: String
	let height: CGFloat
	var rule: Rule? = .light
	var leading: AnyView = AnyView(Color.clear)
	var cells: [AnyView] = []
	var span: AnyView?
}

/// A report table in its card. The first column stays put while the figures
/// scroll sideways under it, and the figures are built a column at a time as
/// they come into view, so a year of weeks never widens the page or slows a
/// scroll. Lines have fixed heights so the two parts line up.
struct PinnedTable: View {
	static let headingHeight: CGFloat = 30
	static let leadingWidth: CGFloat = 200

	var leadingHeading = ""
	let headings: [String]
	let lines: [TableLine]
	@State private var viewport: CGFloat = 0

	var body: some View {
		HStack(alignment: .top, spacing: 0) {
			VStack(alignment: .leading, spacing: 0) {
				heading(leadingHeading, alignment: .leading)
				ForEach(lines) { line in
					rule(line)
					line.leading
						.lineLimit(1)
						.frame(maxWidth: Self.leadingWidth, alignment: .leading)
						.frame(height: line.height, alignment: .leading)
				}
			}
			.fixedSize(horizontal: true, vertical: false)
			ScrollView(.horizontal) {
				LazyHStack(alignment: .top, spacing: 0) {
					ForEach(headings.indices, id: \.self) { column in
						VStack(alignment: .trailing, spacing: 0) {
							heading(headings[column], alignment: .trailing)
							ForEach(lines) { line in
								rule(line)
								(line.cells.indices.contains(column) ? line.cells[column] : AnyView(Color.clear))
									.frame(maxWidth: .infinity, alignment: .trailing)
									.frame(height: line.height)
							}
						}
					}
				}
				.frame(minWidth: viewport, alignment: .trailing)
			}
			.scrollIndicators(.automatic)
			.onScrollGeometryChange(for: CGFloat.self) { $0.containerSize.width } action: { _, width in
				viewport = width
			}
		}
		.background(alignment: .top) { rules }
		.padding(.vertical, Spacing.s1)
		.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
		.clipShape(.rect(cornerRadius: Radius.section))
		.overlay { RoundedRectangle(cornerRadius: Radius.section).strokeBorder(Color.edgeGrouped) }
	}

	private func heading(_ text: String, alignment: Alignment) -> some View {
		Text(text)
			.font(.text(.caption))
			.foregroundStyle(.textTertiary)
			.lineLimit(1)
			.padding(.horizontal, Spacing.s4)
			.frame(height: Self.headingHeight, alignment: alignment)
	}

	/// Each part leaves a rule's room above a line; the rules themselves are
	/// drawn once behind the table, so they run the card's full width.
	private func rule(_ line: TableLine) -> some View {
		Color.clear.frame(height: line.rule == nil ? 0 : 1)
	}

	private var rules: some View {
		VStack(spacing: 0) {
			Color.clear.frame(height: Self.headingHeight)
			ForEach(lines) { line in
				switch line.rule {
				case .light: Rectangle().fill(Color.separator).frame(height: 1)
				case .strong: Rectangle().fill(Color.textTertiary).frame(height: 1)
				case nil: EmptyView()
				}
				(line.span ?? AnyView(Color.clear))
					.frame(maxWidth: .infinity, alignment: .leading)
					.frame(height: line.height)
			}
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
							TableColumn("Payee") { DrilldownPayee(payee: PayeeText($0)) }
							TableColumn("Category") { Text($0.category ?? "") }
							TableColumn("Account") { Text($0.accountName) }
								.width(min: 80, ideal: 100)
							TableColumn("Amount") { row in
								Text(model.format.amount(row.amount, on: .register))
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
		guard let date = CivilDate.date(text) else { return text }
		return date.formatted(.dateTime.month(.abbreviated).day().year())
	}
}

/// A drill-down row's payee as the register draws it: a transfer's arrow in
/// secondary ink before "To Savings" or "From Savings".
private struct DrilldownPayee: View {
	let payee: PayeeText

	var body: some View {
		if let direction = payee.direction {
			Text("\(Text(direction).fontWeight(.semibold).foregroundStyle(.textSecondary)) \(payee.text)")
		} else {
			Text(payee.text)
		}
	}
}
