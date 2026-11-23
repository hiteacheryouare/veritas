import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// Each tour step on its real screen over the stub budget, light and dark,
/// into target/screenshots/tour (D79).
@MainActor
struct TourScreenshotTests {
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/tour")
		.standardizedFileURL

	private func workspace() async -> WorkspaceModel {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		return workspace
	}

	@Test func everyStepPointsAtItsTargetOnItsOwnScreen() async throws {
		let suite = "veritas-tour-shots-\(UUID().uuidString)"
		let defaults = try #require(UserDefaults(suiteName: suite))
		defer { defaults.removePersistentDomain(forName: suite) }
		let workspace = await workspace()
		let budget = await workspace.tourBudget()
		#expect(GuidedTour.steps(for: budget) == TourStep.allCases, "the stub budget has something for every step")

		let tour = GuidedTour(defaults: defaults)
		#expect(tour.offer(budget))
		while let step = tour.current, let destination = tour.destination {
			let anchors = AnchorLog()
			let screen = try await screen(for: destination, in: workspace)
				.overlayPreferenceValue(TourAnchorKey.self) { found in
					Color.clear.onChange(of: Set(found.keys), initial: true) { anchors.steps.formUnion($1) }
				}
				.tourOverlay(tour)
			try render(screen, named: "\(tour.number ?? 0)-\(step.rawValue)", size: CGSize(width: 1100, height: 640))
			#expect(anchors.steps.contains(step), "\(step) has its target on \(destination)")
			tour.next()
		}
		#expect(tour.isFinished)
	}

	@Test func theCardSaysItsStepAndPosition() throws {
		let card = CoachMark(step: .budgetCell, number: 2, count: 5, isLastStep: false, canGoBack: true, back: {}, next: {}, skip: {})
		let texts = try DrawnText.texts(in: card, size: CGSize(width: CoachMark.width, height: 140))
		let drawn = texts.joined(separator: " ")
		#expect(drawn.contains("Click a Budgeted amount"), "\(texts)")
		#expect(texts.map { $0.trimmingCharacters(in: .whitespaces) }.contains("2 of 5"), "\(texts)")
		// A borderless button draws no text into the PDF, so Skip is checked in the screenshots and the UI test.
		#expect(drawn.contains("Back Next"), "\(texts)")
		let last = CoachMark(step: .quickEntry, number: 5, count: 5, isLastStep: true, canGoBack: true, back: {}, next: {}, skip: {})
		#expect(try DrawnText.texts(in: last, size: CGSize(width: CoachMark.width, height: 140)).joined(separator: " ").contains("Done"))
	}

	@Test func cardsAreRenderedForEveryStepOnTheirOwn() throws {
		for (index, step) in TourStep.allCases.enumerated() {
			for side in [CoachMarkPlacement.Side.below, .above] {
				let card = CoachMark(
					step: step, number: index + 1, count: TourStep.allCases.count,
					isLastStep: index == TourStep.allCases.count - 1, canGoBack: index > 0,
					side: side, arrowX: 60, back: {}, next: {}, skip: {}
				)
				try render(card.padding(Spacing.s5), named: "card-\(index + 1)-\(step.rawValue)-\(side == .below ? "below" : "above")")
			}
		}
	}

	private func screen(for destination: Destination, in workspace: WorkspaceModel) async throws -> AnyView {
		switch destination {
		case .screen(.budget):
			let screen = BudgetScreenModel(workspace: workspace)
			await screen.load()
			return AnyView(BudgetScreen(workspace: workspace, screen: screen))
		case .screen(.accounts):
			let model = AccountsModel(workspace: workspace)
			await model.load()
			return AnyView(AccountsScreen(workspace: workspace, model: model))
		case .account(let id):
			let register = RegisterModel(workspace: workspace, scope: .account(accountId: id))
			await register.setFilter { $0.from = "2026-10-10"; $0.to = "2026-11-09" }
			return AnyView(AccountRegisterScreen(workspace: workspace, accountID: id, register: register))
		default:
			Issue.record("the tour never goes to \(destination)")
			return AnyView(EmptyView())
		}
	}

	func render(_ view: some View, named name: String, size: CGSize? = nil, settle: TimeInterval = 0.8) throws {
		try FileManager.default.createDirectory(at: Self.output, withIntermediateDirectories: true)
		for (appearance, suffix) in [(NSAppearance.Name.aqua, "light"), (.darkAqua, "dark")] {
			let host = NSHostingView(rootView: view.tint(Color.accent).background(.surfaceWindow))
			host.frame.size = size ?? host.fittingSize
			let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
			window.appearance = NSAppearance(named: appearance)
			window.contentView = host
			host.layoutSubtreeIfNeeded()
			RunLoop.main.run(until: .now.addingTimeInterval(settle))
			if size == nil { host.frame.size = host.fittingSize }
			host.layoutSubtreeIfNeeded()
			let bitmap = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
			host.cacheDisplay(in: host.bounds, to: bitmap)
			let png = try #require(bitmap.representation(using: .png, properties: [:]))
			try png.write(to: Self.output.appending(path: "\(name)-\(suffix).png"))
		}
	}
}

@MainActor
private final class AnchorLog {
	var steps: Set<TourStep> = []
}

/// Where the coach mark goes, and that its words stay readable (WCAG AA, 4.5:1).
struct CoachMarkPlacementTests {
	let card = CGSize(width: 320, height: 110)
	let container = CGSize(width: 1000, height: 600)

	@Test func sitsBelowItsTargetWhenThereIsRoom() {
		let target = CGRect(x: 100, y: 40, width: 200, height: 40)
		let placement = CoachMarkPlacement.place(card, pointingAt: target, in: container)
		#expect(placement.side == .below)
		#expect(placement.frame.minY == target.maxY + CoachMarkPlacement.gap)
		#expect(placement.frame.minX + placement.arrowX == target.midX, "the arrow points at the target's middle")
	}

	@Test func goesAboveATargetNearTheBottom() {
		let target = CGRect(x: 400, y: 540, width: 100, height: 30)
		let placement = CoachMarkPlacement.place(card, pointingAt: target, in: container)
		#expect(placement.side == .above)
		#expect(placement.frame.maxY == target.minY - CoachMarkPlacement.gap)
	}

	@Test func staysInsideTheMarginsAndKeepsTheArrowOnTheCard() {
		let target = CGRect(x: 960, y: 10, width: 30, height: 20)
		let placement = CoachMarkPlacement.place(card, pointingAt: target, in: container)
		#expect(placement.frame.maxX == container.width - CoachMarkPlacement.margin)
		#expect(placement.arrowX <= card.width - CoachMarkShape.arrowWidth / 2)
	}

	@Test func withNoTargetItSitsAtTheBottomWithNoArrow() {
		let missing = CoachMarkPlacement.place(card, pointingAt: nil, in: container)
		#expect(missing.side == .free)
		#expect(missing.frame.maxY == container.height - CoachMarkPlacement.margin)
		let scrolledAway = CoachMarkPlacement.place(card, pointingAt: CGRect(x: 100, y: 900, width: 50, height: 20), in: container)
		#expect(scrolledAway.side == .free)
	}

	@Test func textOnTheCardMeetsAAContrastInLightAndDark() {
		for text in [ColorToken.textPrimary, .textSecondary, .accent] {
			#expect(Self.contrast(text.light, ColorToken.surfaceRaised.light) >= 4.5)
			#expect(Self.contrast(text.dark, ColorToken.surfaceRaised.dark) >= 4.5)
		}
	}

	static func contrast(_ a: RGBA, _ b: RGBA) -> Double {
		func luminance(_ color: RGBA) -> Double {
			func channel(_ value: Double) -> Double {
				value <= 0.03928 ? value / 12.92 : pow((value + 0.055) / 1.055, 2.4)
			}
			return 0.2126 * channel(color.red) + 0.7152 * channel(color.green) + 0.0722 * channel(color.blue)
		}
		let (lighter, darker) = (max(luminance(a), luminance(b)), min(luminance(a), luminance(b)))
		return (lighter + 0.05) / (darker + 0.05)
	}
}
