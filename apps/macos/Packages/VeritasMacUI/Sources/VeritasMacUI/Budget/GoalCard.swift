import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// A goal on the Budget overview: a ring, the progress and where it stands.
struct GoalCard: View {
	let goal: GoalSummary
	let format: MoneyFormat
	@Environment(\.accessibilityReduceMotion) private var reduceMotion

	var body: some View {
		HStack(spacing: Spacing.s3) {
			ZStack {
				Circle().stroke(Color.track, lineWidth: 5)
				Circle()
					.trim(from: 0, to: goal.progress)
					.stroke(Color(tone.fill), style: StrokeStyle(lineWidth: 5, lineCap: .round))
					.rotationEffect(.degrees(-90))
			}
			.frame(width: Size.ring, height: Size.ring)
			.padding(2.5)
			.motion(.fast, value: goal.progress)
			.accessibilityHidden(true)
			VStack(alignment: .leading, spacing: 2) {
				Text(goal.name)
					.font(.text(.body, weight: .semibold))
				HStack(spacing: 4) {
					Text(format.wholeCurrency(goal.available)).font(.figures(.body, weight: .semibold))
					Text("of")
					Text(format.wholeCurrency(goal.target)).font(.figures(.body, weight: .semibold))
				}
				.font(.text(.body))
				Label {
					Text(note)
				} icon: {
					Image(systemName: tone.symbol ?? "checkmark.circle").imageScale(.small)
				}
				.font(.text(.caption))
				.foregroundStyle(Color(tone.text))
				.lineLimit(1)
				.fixedSize()
				.padding(.top, 1)
			}
			Spacer(minLength: 0)
		}
		.padding(.horizontal, Spacing.s4)
		.padding(.vertical, Spacing.s3)
		.frame(minWidth: 280)
		.fixedSize()
		.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
		.overlay(RoundedRectangle(cornerRadius: Radius.section).strokeBorder(Color.edgeGrouped))
		.accessibilityElement(children: .combine)
		.containerIdentifier("goal.\(goal.id)")
	}

	private var tone: VeritasDesign.MoneyTone {
		switch goal.status {
		case .behind: .low
		default: .available
		}
	}

	private var note: String {
		switch goal.status {
		case .onTrack:
			goal.targetMonth.map { "On track for \(Self.monthName($0))" } ?? "On track"
		case .behind(let monthly):
			"Behind, needs \(format.wholeCurrency(monthly)) a month"
		case .reached:
			"Reached"
		case .spent:
			"Reached → Spent"
		}
	}

	static func monthName(_ month: Month) -> String {
		var parts = DateComponents()
		parts.year = Int(month.year)
		parts.month = Int(month.month)
		parts.day = 1
		return Calendar.current.date(from: parts)?.formatted(.dateTime.month(.wide)) ?? ""
	}
}
