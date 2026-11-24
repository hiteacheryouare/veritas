import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasKit
@testable import VeritasMacUI

/// A long answer streams into the panel in an offscreen window, and the
/// panel stays steady: the view only ever scrolls down, and the reply
/// bubble never narrows or shrinks.
@MainActor
struct AssistantStreamingTests {
	/// Hands the test the stream, so it decides when each word arrives.
	final class Feed: AssistantAnswering {
		private(set) var continuation: AsyncThrowingStream<String, any Error>.Continuation?

		func answer(_ question: String) -> AsyncThrowingStream<String, any Error> {
			let (stream, continuation) = AsyncThrowingStream.makeStream(of: String.self, throwing: (any Error).self)
			self.continuation = continuation
			return stream
		}

		func reset() {}
	}

	static func descendants(of view: NSView) -> [NSView] {
		view.subviews.flatMap { [$0] + descendants(of: $0) }
	}

	/// The latest frame each reply bubble reported.
	final class Frames {
		var latest: [UUID: CGRect] = [:]
	}

	private func settle(_ seconds: TimeInterval = 0.05) {
		RunLoop.main.run(until: .now.addingTimeInterval(seconds))
	}

	@Test func aLongAnswerScrollsOnlyDownAndItsBubbleNeverShrinks() async throws {
		let feed = Feed()
		let model = AssistantPanelModel(makeAnswerer: { _ in feed }) {
			AssistantContext(queries: StubReadOnlyQueries(), format: MoneyFormat(), today: "2026-11-17")
		}
		let reported = Frames()
		let host = NSHostingView(rootView: AssistantPanel(model: model).frame(width: 340, height: 520)
			.environment(\.assistantReplyFrame) { id, frame in reported.latest[id] = frame })
		host.frame = CGRect(x: 0, y: 0, width: 340, height: 520)
		let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
		window.contentView = host
		window.orderBack(nil)
		defer { window.orderOut(nil) }
		settle(0.3)

		model.draft = "How did October go?"
		model.ask()
		for _ in 0..<10_000 where feed.continuation == nil { await Task.yield() }
		let stream = try #require(feed.continuation)
		settle()

		let scroll = try #require(Self.descendants(of: host).compactMap { $0 as? NSScrollView }.first)
		func offset() -> CGFloat { scroll.contentView.bounds.origin.y }
		func bubble() -> CGRect? {
			model.exchanges.first.flatMap { reported.latest[$0.id] }
		}

		let words = (1...260).map { "word\($0)" } + ["**Total**", "$2,489.35."]
		var text = ""
		var offsets: [CGFloat] = [offset()]
		var frames: [CGRect] = []
		let waiting = try #require(bubble(), "the reply bubble is there while the dots show")
		frames.append(waiting)
		for (index, word) in words.enumerated() {
			text += text.isEmpty ? word : " \(word)"
			stream.yield(text)
			if index % 4 == 0 {
				await Task.yield()
				settle(0.04)
				offsets.append(offset())
				if let frame = bubble() { frames.append(frame) }
			}
		}
		stream.finish()
		await model.settle()
		settle(0.2)
		offsets.append(offset())
		if let frame = bubble() { frames.append(frame) }

		#expect(model.exchanges.first?.answer == text)
		#expect((offsets.last ?? 0) > (offsets.first ?? 0) + 100, "the long answer scrolled the panel: \(offsets)")
		for (earlier, later) in zip(offsets, offsets.dropFirst()) {
			#expect(later >= earlier - 0.5, "the view never scrolls up while text arrives: \(offsets)")
		}
		#expect(frames.count > 10)
		for (earlier, later) in zip(frames, frames.dropFirst()) {
			#expect(abs(later.width - earlier.width) < 0.5, "the bubble's width never changes: \(earlier.width) then \(later.width)")
			#expect(later.height >= earlier.height - 0.5, "the bubble never gets shorter: \(earlier.height) then \(later.height)")
		}
	}
}

/// Reads nothing: the streaming test never calls a tool.
struct StubReadOnlyQueries: ReadOnlyBudgetQueries {
	struct Unused: Error {}
	func netProfitPeriod(first: String, last: String) async throws -> NetProfitPeriod { throw Unused() }
	func budgetHabits(today: String) async throws -> BudgetHabits { throw Unused() }
	func spendingByCategory(first: String, last: String) async throws -> CategoryBreakdown { throw Unused() }
	func balanceSheet(asOf: String) async throws -> BalanceSheet { throw Unused() }
	func cashFlowStatement(first: String, last: String) async throws -> CashFlowColumn { throw Unused() }
	func budgetMonth(_ month: Month, today: String) async throws -> BudgetMonth { throw Unused() }
	func accounts() async throws -> AccountList { throw Unused() }
	func goals(today: String) async throws -> GoalList { throw Unused() }
	func taxSummary(year: Int32) async throws -> TaxSummary { throw Unused() }
	func customReports() async throws -> [CustomReportItem] { throw Unused() }
	func customReport(_ id: String, today: String) async throws -> CustomReportView { throw Unused() }
}
