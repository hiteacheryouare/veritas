import Accessibility
import SwiftUI
import VeritasDesign
import VeritasKit

/// The 12 month initials along a chart's axis, January first. The months of
/// a year are one of the copy rule's fixed constants, so an empty slot after
/// the period still shows its initial.
private let monthInitials = localGregorian.veryShortStandaloneMonthSymbols

/// A Gregorian calendar in the user's language, so month names read as they
/// do elsewhere in the app. A bare calendar has no locale and prints "M01".
private let localGregorian: Calendar = {
	var calendar = Calendar(identifier: .gregorian)
	calendar.locale = .current
	return calendar
}()

/// The shape of the year (FC-8's monthly series): a column per month by the
/// core's share of total out, the biggest month in the accent, a refund month
/// hatched below the line, "n/a" where the core gave no share, and the
/// typical month as a dashed line. Columns fade in at full height: a column
/// growing would show amounts that aren't real.
struct WrappedColumnChart: View {
	let page: WrappedPage
	let palette: WrappedPalette

	static let gap: CGFloat = 16
	static let annotationSpace: CGFloat = 44
	static let axisSpace: CGFloat = 28

	var body: some View {
		GeometryReader { proxy in
			let width = (proxy.size.width - Self.gap * 11) / 12
			let shares = page.columns.compactMap(\.share)
			let up = CGFloat(max(shares.max() ?? 0, page.reference?.share ?? 0, 0))
			let down = CGFloat(max(-(shares.min() ?? 0), 0))
			let plot = proxy.size.height - Self.annotationSpace - Self.axisSpace
			let scale = up + down > 0 ? plot / (up + down) : 0
			let baseline = Self.annotationSpace + up * scale
			let x = { (slot: Int) in CGFloat(slot) * (width + Self.gap) }
			ZStack(alignment: .topLeading) {
				Rectangle()
					.fill(palette.ink.opacity(0.4))
					.frame(width: proxy.size.width, height: 0.5)
					.offset(y: baseline)
				if let reference = page.reference {
					let y = baseline - CGFloat(reference.share) * scale
					Path { path in
						path.move(to: CGPoint(x: 0, y: y))
						path.addLine(to: CGPoint(x: proxy.size.width, y: y))
					}
					.stroke(palette.inkSecondary, style: StrokeStyle(lineWidth: 0.5, dash: [4, 3]))
					Text(reference.label)
						.font(WrappedType.annotation)
						.foregroundStyle(palette.inkSecondary)
						.fixedSize()
						.frame(width: proxy.size.width, alignment: .trailing)
						.offset(y: y - 18)
				}
				ForEach(Array(page.columns.enumerated()), id: \.element.id) { index, column in
					columnMark(column, width: width, baseline: baseline, scale: scale, plot: plot)
						.offset(x: x(column.slot))
						.staged(5 + index)
				}
				ForEach(0..<12, id: \.self) { slot in
					Text(monthInitials[slot])
						.font(WrappedType.axis)
						.foregroundStyle(page.columns.contains { $0.slot == slot } ? palette.inkSecondary : palette.inkSecondary.opacity(0.5))
						.frame(width: width)
						.offset(x: x(slot), y: proxy.size.height - Self.axisSpace + 8)
				}
				ForEach(Array(page.annotations.enumerated()), id: \.offset) { _, annotation in
					if case .column(let slot) = annotation.target, let column = page.columns.first(where: { $0.slot == slot }) {
						let share = CGFloat(column.share ?? 0)
						let top = share >= 0 ? baseline - share * scale : baseline - share * scale + 4
						WrappedLeader(text: annotation.text, palette: palette, flipsLeft: slot >= 8, below: share < 0)
							.offset(x: x(slot) + width / 2, y: share >= 0 ? top : top)
							.staged(Motion.stageSteps)
					}
				}
			}
		}
		.frame(height: 440)
		.accessibilityElement(children: .ignore)
		.accessibilityLabel(page.spoken ?? "")
		.accessibilityChartDescriptor(WrappedColumnDescriptor(columns: page.columns))
		.accessibilityIdentifier("wrapped.columns")
	}

	@ViewBuilder private func columnMark(_ column: WrappedPage.Column, width: CGFloat, baseline: CGFloat, scale: CGFloat, plot: CGFloat) -> some View {
		if let share = column.share {
			let height = max(CGFloat(abs(share)) * scale, share == 0 ? 0 : 2)
			Group {
				if column.isRefund {
					WrappedHatch(color: palette.bar)
						.overlay(Rectangle().strokeBorder(palette.bar, lineWidth: 1))
				} else {
					Rectangle().fill(column.isBiggest ? palette.accent : palette.bar)
				}
			}
			.frame(width: width, height: height)
			.offset(y: share >= 0 ? baseline - height : baseline)
		} else {
			Text(WrappedWords.notApplicable)
				.font(WrappedType.axis)
				.foregroundStyle(palette.inkSecondary)
				.frame(width: width)
				.offset(y: baseline - 18)
		}
	}
}

/// Net worth at each month end: a line through the core's month ends, the
/// last one marked in the accent.
struct WrappedLineChart: View {
	let page: WrappedPage
	let palette: WrappedPalette

	static let axisSpace: CGFloat = 28
	static let headroom: CGFloat = 40

	var body: some View {
		GeometryReader { proxy in
			let values = page.points.map { CGFloat($0.value) }
			let low = values.min() ?? 0, high = values.max() ?? 0
			let plot = proxy.size.height - Self.axisSpace - Self.headroom * 2
			let span = high - low
			let point = { (item: WrappedPage.Point) -> CGPoint in
				let x = proxy.size.width * CGFloat(item.slot) / 11
				let y = Self.headroom + (span > 0 ? (high - CGFloat(item.value)) / span * plot : plot / 2)
				return CGPoint(x: x, y: y)
			}
			ZStack(alignment: .topLeading) {
				Path { path in
					for (index, item) in page.points.enumerated() {
						if index == 0 { path.move(to: point(item)) } else { path.addLine(to: point(item)) }
					}
				}
				.stroke(palette.bar, style: StrokeStyle(lineWidth: 2, lineCap: .round, lineJoin: .round))
				.staged(5)
				if let last = page.points.last {
					Circle()
						.fill(palette.accent)
						.frame(width: 8, height: 8)
						.position(point(last))
						.staged(6)
				}
				ForEach(Array(page.annotations.enumerated()), id: \.offset) { _, annotation in
					if case .point(let index) = annotation.target, page.points.indices.contains(index) {
						let at = point(page.points[index])
						let isLast = index == page.points.count - 1
						WrappedLeader(text: annotation.text, palette: palette, flipsLeft: page.points[index].slot >= 8, below: !isLast)
							.offset(x: at.x, y: isLast ? at.y - 6 : at.y + 6)
							.staged(Motion.stageSteps)
					}
				}
				ForEach(0..<12, id: \.self) { slot in
					Text(monthInitials[slot])
						.font(WrappedType.axis)
						.foregroundStyle(palette.inkSecondary)
						.fixedSize()
						.position(x: proxy.size.width * CGFloat(slot) / 11, y: proxy.size.height - Self.axisSpace / 2)
				}
			}
		}
		.frame(height: 300)
		.padding(.horizontal, 8)
		.accessibilityElement(children: .ignore)
		.accessibilityLabel(page.spoken ?? "")
		.accessibilityChartDescriptor(WrappedLineDescriptor(points: page.points))
		.accessibilityIdentifier("wrapped.line")
	}
}

/// An annotation: a hairline elbow from the mark and its words at the end.
private struct WrappedLeader: View {
	let text: String
	let palette: WrappedPalette
	/// Near the right edge the words run to the left of the mark.
	let flipsLeft: Bool
	/// Under the mark rather than above it.
	let below: Bool

	var body: some View {
		ZStack(alignment: .topLeading) {
			Path { path in
				let rise: CGFloat = below ? 14 : -14
				path.move(to: .zero)
				path.addLine(to: CGPoint(x: 0, y: rise))
				path.addLine(to: CGPoint(x: flipsLeft ? -10 : 10, y: rise))
			}
			.stroke(palette.inkSecondary, lineWidth: 0.5)
			Text(text)
				.font(WrappedType.annotation)
				.foregroundStyle(palette.inkSecondary)
				.fixedSize()
				.frame(width: 260, alignment: flipsLeft ? .trailing : .leading)
				.offset(x: flipsLeft ? -274 : 14, y: below ? 7 : -21)
		}
		.accessibilityHidden(true)
	}
}

/// Diagonal hatching, the refund month's non-color signal (the same as an
/// overspent drain bar's).
private struct WrappedHatch: View {
	let color: Color

	var body: some View {
		Canvas { context, size in
			var stripes = Path()
			var x = -size.height
			while x < size.width {
				stripes.move(to: CGPoint(x: x, y: size.height))
				stripes.addLine(to: CGPoint(x: x + size.height, y: 0))
				x += 5
			}
			context.stroke(stripes, with: .color(color), lineWidth: 1)
		}
		.clipped()
	}
}

/// The year on one page, a small grid per month: a no-spend date in the
/// secondary accent, a spent date in the soft bar color, and a date outside
/// the evaluated range left as an outline (FC-8 N3's three states).
struct WrappedCalendar: View {
	let calendar: WrappedPage.Calendar
	let spoken: String
	let palette: WrappedPalette

	private static let gregorian = localGregorian
	private static let cell: CGFloat = 9
	private static let spacing: CGFloat = 2

	var body: some View {
		Grid(horizontalSpacing: 24, verticalSpacing: 20) {
			ForEach(0..<3, id: \.self) { row in
				GridRow {
					ForEach(0..<4, id: \.self) { column in
						month(row * 4 + column + 1)
					}
				}
			}
		}
		.accessibilityElement(children: .ignore)
		.accessibilityLabel(([spoken] + calendar.months).joined(separator: ". "))
		.accessibilityIdentifier("wrapped.calendar")
		.staged(5)
	}

	private func month(_ month: Int) -> some View {
		let year = Int(calendar.year)
		let first = DateComponents(calendar: Self.gregorian, year: year, month: month, day: 1).date ?? .distantPast
		let days = Self.gregorian.range(of: .day, in: .month, for: first)?.count ?? 30
		let lead = (Self.gregorian.component(.weekday, from: first) - Self.gregorian.firstWeekday + 7) % 7
		return VStack(alignment: .leading, spacing: 6) {
			Text(Self.gregorian.shortStandaloneMonthSymbols[month - 1])
				.font(WrappedType.axis)
				.foregroundStyle(palette.inkSecondary)
			LazyVGrid(columns: Array(repeating: GridItem(.fixed(Self.cell), spacing: Self.spacing), count: 7), alignment: .leading, spacing: Self.spacing) {
				ForEach(0..<(lead + days), id: \.self) { index in
					if index < lead {
						Color.clear.frame(width: Self.cell, height: Self.cell)
					} else {
						day(String(format: "%04d-%02d-%02d", year, month, index - lead + 1))
					}
				}
			}
		}
	}

	@ViewBuilder private func day(_ date: String) -> some View {
		let evaluated = calendar.first.map { $0 <= date } == true && calendar.last.map { date <= $0 } == true
		if calendar.dates.contains(date) {
			Rectangle().fill(palette.accentSecondary).frame(width: Self.cell, height: Self.cell)
		} else if evaluated {
			Rectangle().fill(palette.barSoft).frame(width: Self.cell, height: Self.cell)
		} else {
			Rectangle().strokeBorder(palette.rule, lineWidth: 0.5).frame(width: Self.cell, height: Self.cell)
		}
	}
}

/// The column chart as an audio graph: each month and its share of total out.
private struct WrappedColumnDescriptor: AXChartDescriptorRepresentable {
	let columns: [WrappedPage.Column]

	func makeChartDescriptor() -> AXChartDescriptor {
		let drawn = columns.filter { $0.share != nil }
		let values = drawn.compactMap(\.share).map { Double($0) / 10 }
		let x = AXCategoricalDataAxisDescriptor(title: "Month", categoryOrder: drawn.map(\.name))
		let y = AXNumericDataAxisDescriptor(title: "Share of total out", range: (min(values.min() ?? 0, 0))...(max(values.max() ?? 0, 1)), gridlinePositions: []) { "\($0.formatted(.number.precision(.fractionLength(1))))%" }
		let series = AXDataSeriesDescriptor(name: "Spending", isContinuous: false, dataPoints: zip(drawn, values).map { AXDataPoint(x: $0.name, y: $1) })
		return AXChartDescriptor(title: "Spending by month", summary: nil, xAxis: x, yAxis: y, additionalAxes: [], series: [series])
	}
}

/// The net worth line as an audio graph.
private struct WrappedLineDescriptor: AXChartDescriptorRepresentable {
	let points: [WrappedPage.Point]

	func makeChartDescriptor() -> AXChartDescriptor {
		let values = points.map { Double($0.value) }
		let x = AXCategoricalDataAxisDescriptor(title: "Month", categoryOrder: points.map(\.name))
		let y = AXNumericDataAxisDescriptor(title: "Net worth", range: (values.min() ?? 0)...(max(values.max() ?? 0, (values.min() ?? 0) + 1)), gridlinePositions: []) { _ in "" }
		let series = AXDataSeriesDescriptor(name: "Net worth", isContinuous: true, dataPoints: zip(points, points.map(\.spoken)).map { AXDataPoint(x: $0.name, y: Double($0.value), label: $1) })
		return AXChartDescriptor(title: "Net worth at each month end", summary: nil, xAxis: x, yAxis: y, additionalAxes: [], series: [series])
	}
}
