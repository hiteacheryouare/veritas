import Charts
import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Net worth from today to the end of the 12th month: the baseline dashed,
/// each compared what-if in its own color, and the baseline's band drawn
/// through the four horizons (design/mockups/forecast.html).
struct NetWorthForecastChart: View {
	let model: ForecastModel
	let forecast: Forecast

	private struct Point: Identifiable {
		let series: Int
		let date: Date
		let value: Double
		var id: String { "\(series)-\(date.timeIntervalSince1970)" }
	}

	private struct BandPoint: Identifiable {
		let date: Date
		let low: Double
		let high: Double
		var id: Double { date.timeIntervalSince1970 }
	}

	private var today: Date { CivilDate.date(forecast.today) ?? .now }

	private func value(_ amount: Int64) -> Double {
		model.format.chartValue(amount, on: .register)
	}

	private static func end(of month: Month) -> Date? {
		CivilDate.date(month.lastDay)
	}

	private var points: [Point] {
		model.projections.enumerated().flatMap { index, projection in
			[Point(series: index, date: today, value: value(forecast.netWorthToday))]
				+ projection.months.compactMap { month in
					Self.end(of: month.month).map { Point(series: index, date: $0, value: value(month.netWorth)) }
				}
		}
	}

	private var band: [BandPoint] {
		let horizons = forecast.baseline.horizons.compactMap { horizon -> BandPoint? in
			guard let low = horizon.bandLow, let high = horizon.bandHigh, let date = Self.end(of: horizon.month) else { return nil }
			return BandPoint(date: date, low: value(low), high: value(high))
		}
		guard !horizons.isEmpty else { return [] }
		let start = value(forecast.netWorthToday)
		return [BandPoint(date: today, low: start, high: start)] + horizons
	}

	private var selected: [Point] {
		model.projections.enumerated().compactMap { index, projection in
			guard let point = model.point(projection), let date = Self.end(of: point.month) else { return nil }
			return Point(series: index, date: date, value: value(point.netWorth))
		}
	}

	var body: some View {
		VStack(alignment: .trailing, spacing: Spacing.s2) {
			if !band.isEmpty {
				HStack(spacing: 6) {
					RoundedRectangle(cornerRadius: 3).fill(Color.chartBand).frame(width: 22, height: 12)
					Text("Likely range")
				}
				.font(.text(.caption))
				.foregroundStyle(.textSecondary)
				.accessibilityHidden(true)
			}
			Chart {
				ForEach(band) { point in
					AreaMark(x: .value("Date", point.date), yStart: .value("Low", point.low), yEnd: .value("High", point.high), series: .value("Series", "band"))
						.foregroundStyle(Color.chartBand)
						.interpolationMethod(.linear)
				}
				RuleMark(x: .value("Today", today))
					.foregroundStyle(Color.textTertiary)
					.lineStyle(StrokeStyle(lineWidth: 1, dash: [2, 3]))
					.annotation(position: .top, alignment: .leading) {
						Text("Today").font(.text(.caption)).foregroundStyle(.textSecondary)
					}
				ForEach(points) { point in
					LineMark(x: .value("Date", point.date), y: .value("Net worth", point.value), series: .value("Series", "\(point.series)"))
						.foregroundStyle(ForecastColors.color(point.series))
						.lineStyle(StrokeStyle(lineWidth: 2.25, lineCap: .round, lineJoin: .round, dash: point.series == 0 ? [6, 4] : []))
				}
				ForEach(selected) { point in
					PointMark(x: .value("Date", point.date), y: .value("Net worth", point.value))
						.foregroundStyle(ForecastColors.color(point.series))
						.symbolSize(36)
				}
			}
			.chartLegend(.hidden)
			.chartYScale(domain: .automatic(includesZero: false))
			.chartYAxis {
				AxisMarks(position: .leading) { mark in
					AxisGridLine().foregroundStyle(Color.chartGrid)
					AxisValueLabel {
						if let amount = mark.as(Double.self) {
							Text(model.format.axisLabel(amount)).font(.figures(.caption)).foregroundStyle(.textTertiary)
						}
					}
				}
			}
			.chartXAxis {
				AxisMarks(values: .stride(by: .month, count: 2)) { mark in
					AxisValueLabel(format: .dateTime.month(.abbreviated))
						.foregroundStyle(Color.textSecondary)
				}
			}
			.frame(height: 280)
			.accessibilityLabel("Net worth forecast for 12 months")
			.accessibilityIdentifier("forecast.chart")
		}
	}
}

enum ForecastColors {
	/// 0 is the baseline, then the compared what-ifs in order.
	static func color(_ series: Int) -> Color {
		switch series {
		case 0: .scenarioBaseline
		case 1: .scenario1
		case 2: .scenario2
		default: .scenario3
		}
	}
}

/// The line a series draws, as a legend swatch: the baseline dashed.
struct SeriesSwatch: View {
	let series: Int

	var body: some View {
		Canvas { context, size in
			var path = Path()
			path.move(to: CGPoint(x: 0, y: size.height / 2))
			path.addLine(to: CGPoint(x: size.width, y: size.height / 2))
			context.stroke(path, with: .color(ForecastColors.color(series)), style: StrokeStyle(lineWidth: 2.5, dash: series == 0 ? [5, 3] : []))
		}
		.frame(width: 22, height: 6)
		.accessibilityHidden(true)
	}
}
