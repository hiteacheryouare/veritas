import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// A goal on the Budget overview: a ring, the progress and where it stands.
struct GoalCard: View {
	let goal: GoalSummary
	let format: MoneyFormat

	private var words: GoalWords { GoalWords(goal, format: format) }

	var body: some View {
		HStack(spacing: Spacing.s3) {
			GoalRing(goal: goal, tone: words.tone, size: Size.ring, lineWidth: 5)
			VStack(alignment: .leading, spacing: 2) {
				Text(goal.name)
					.font(.text(.body, weight: .semibold))
				GoalProgress(goal: goal, format: format)
				Label {
					Text(words.note)
				} icon: {
					Image(systemName: words.tone.symbol ?? "checkmark.circle").imageScale(.small)
				}
				.font(.text(.caption))
				.foregroundStyle(Color(words.tone.text))
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
		.accessibilityElement(children: .ignore)
		.accessibilityLabel("\(goal.name), \(format.wholeCurrency(goal.shownProgress)) of \(format.wholeCurrency(goal.target)), \(words.note)")
		.accessibilityIdentifier("goal.\(goal.id)")
	}
}

struct GoalProgress: View {
	let goal: GoalSummary
	let format: MoneyFormat

	var body: some View {
		HStack(spacing: 4) {
			Text(format.wholeCurrency(goal.shownProgress)).font(.figures(.body, weight: .semibold))
			Text("of")
			Text(format.wholeCurrency(goal.target)).font(.figures(.body, weight: .semibold))
		}
		.font(.text(.body))
	}
}

struct GoalRing: View {
	let goal: GoalSummary
	let tone: VeritasDesign.MoneyTone
	let size: CGFloat
	let lineWidth: CGFloat
	var showsPercent = false

	var body: some View {
		ZStack {
			Circle().stroke(Color.track, lineWidth: lineWidth)
			Circle()
				.trim(from: 0, to: goal.ringProgress)
				.stroke(Color(tone == .neutral ? ColorToken.moneyNeutral : tone.fill), style: StrokeStyle(lineWidth: lineWidth, lineCap: .round))
				.rotationEffect(.degrees(-90))
			if showsPercent {
				Text(goal.ringProgress, format: .percent.precision(.fractionLength(0)))
					.font(.figures(.title, weight: .bold))
			}
		}
		.frame(width: size, height: size)
		.padding(lineWidth / 2)
		.motion(.fast, value: goal.ringProgress)
		.accessibilityHidden(true)
	}
}
