import Testing
import VeritasKit
@testable import VeritasMacUI

/// The assistant shows "Mo Nee" and VoiceOver says "Money"; the typing dots
/// pulse in turn, and hold still with Reduce Motion.
struct AssistantPanelSpeechTests {
	@Test func theNameIsShownOneWayAndSpokenAnother() {
		#expect(AssistantName.shown == "Mo Nee")
		#expect(AssistantName.spoken == "Money")
		#expect(AssistantSpeech.reply(answer: "Spending, October 2026: $2,489.35", failure: nil) == "Money: Spending, October 2026: $2,489.35")
		#expect(AssistantSpeech.reply(answer: "", failure: nil) == "Money", "while waiting, the reply is just its sender; its value says Answering")
		#expect(AssistantSpeech.reply(answer: "", failure: "The model is busy. Ask again in a moment.") == "Money: The model is busy. Ask again in a moment.")
		#expect(AssistantSpeech.question("What did I spend?") == "You: What did I spend?", "your own messages carry no assistant name")
		#expect(AssistantSpeech.answering == "Answering")
	}

	@Test func withReduceMotionTheDotsHoldStill() {
		let still = (0..<TypingDots.count).map { TypingDots.opacity(index: $0, time: nil) }
		#expect(Set(still) == [TypingDots.still])
	}

	@Test func withMotionTheDotsPulseOneAfterAnother() {
		let times = Array(stride(from: 0.0, to: TypingDots.cycle, by: TypingDots.cycle / 240))
		for index in 0..<TypingDots.count {
			let values = times.map { TypingDots.opacity(index: index, time: $0) }
			#expect(values.allSatisfy { (0.3...1.0).contains($0) })
			#expect((values.max() ?? 0) - (values.min() ?? 0) > 0.6, "dot \(index) pulses")
		}
		let peaks = (0..<TypingDots.count).map { index in
			times.max { TypingDots.opacity(index: index, time: $0) < TypingDots.opacity(index: index, time: $1) } ?? 0
		}
		let step = TypingDots.cycle / Double(TypingDots.count + 1)
		for index in 1..<TypingDots.count {
			let gap = (peaks[index] - peaks[index - 1] + TypingDots.cycle).truncatingRemainder(dividingBy: TypingDots.cycle)
			#expect(abs(gap - step) < TypingDots.cycle / 100, "dot \(index) peaks a beat after dot \(index - 1): \(peaks)")
		}
	}
}
