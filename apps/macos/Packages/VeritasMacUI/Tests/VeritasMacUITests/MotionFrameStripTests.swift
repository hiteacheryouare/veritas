import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// Frame strips of the main transitions into target/screenshots/motion, for
/// review. Each frame is two real screens composited at the opacity and
/// offset the motion tokens give at that moment, so the strip shows the
/// timing the app runs, sampled every 40 ms.
@MainActor
struct MotionFrameStripTests {
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/motion")
		.standardizedFileURL
	static let size = CGSize(width: 1000, height: 640)
	static let times: [Double] = [0, 0.04, 0.08, 0.12, 0.16, 0.20, 0.24, 0.32]

	private func workspace() async -> WorkspaceModel {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		return workspace
	}

	@Test func pageFadeThrough() async throws {
		let workspace = await workspace()
		let budget = BudgetScreenModel(workspace: workspace, month: StubBudget.month)
		await budget.load()
		let bills = BillsModel(workspace: workspace)
		await bills.load()
		let from = try image(BudgetScreen(workspace: workspace, screen: budget))
		let to = try image(BillsScreen(workspace: workspace, model: bills))
		try strip(named: "page-fade-through", from: from, to: to, transition: .page)
		#expect(Motion.pageOut.progress(at: Motion.Duration.pageExit) == 1, "the old page is gone before the new one finishes")
	}

	@Test func pushIntoARegister() async throws {
		let workspace = await workspace()
		let accounts = AccountsModel(workspace: workspace)
		await accounts.load()
		let register = RegisterModel(workspace: workspace, scope: .account(accountId: "checking"))
		await register.setFilter { $0.from = "2026-10-10"; $0.to = "2026-11-09" }
		let from = try image(AccountsScreen(workspace: workspace, model: accounts))
		let to = try image(AccountRegisterScreen(workspace: workspace, accountID: "checking", register: register), settle: 0.8)
		try strip(named: "push-register", from: from, to: to, transition: .push)
	}

	@Test func monthChangeCrossFadesTheFigures() async throws {
		let workspace = await workspace()
		let budget = BudgetScreenModel(workspace: workspace, month: StubBudget.month)
		await budget.load()
		let from = try image(BudgetScreen(workspace: workspace, screen: budget))
		await budget.previousMonth()
		let to = try image(BudgetScreen(workspace: workspace, screen: budget))
		let fade = MotionTransition("month", insertion: .init(.fast), removal: .init(.fast))
		try strip(named: "month-change", from: from, to: to, transition: fade)
	}

	@Test func curves() throws {
		let motions: [Motion] = [.fast, .sheet, .pageIn, .pageOut, .settle, .morph, .turn]
		let width = 640.0, height = 320.0, inset = 40.0
		let image = NSImage(size: NSSize(width: width, height: height), flipped: false) { _ in
			NSColor.white.setFill()
			NSRect(x: 0, y: 0, width: width, height: height).fill()
			let span = 0.6
			for (index, motion) in motions.enumerated() {
				let path = NSBezierPath()
				for step in 0...120 {
					let time = span * Double(step) / 120
					let point = NSPoint(x: inset + (width - 2 * inset) * time / span, y: inset + (height - 2 * inset) * motion.progress(at: time))
					if step == 0 { path.move(to: point) } else { path.line(to: point) }
				}
				let shade = NSColor(white: 0.1 + 0.11 * Double(index), alpha: 1)
				shade.setStroke()
				path.lineWidth = 1.5
				path.stroke()
				Self.label(motion.name, at: NSPoint(x: width - inset - 60, y: height - inset - 14 * Double(index + 1)), color: shade)
			}
			Self.label("0 to 600 ms", at: NSPoint(x: inset, y: 12))
			return true
		}
		try write(image, named: "curves")
	}

	// MARK: Drawing

	private func image(_ view: some View, settle: TimeInterval = 0.4) throws -> NSImage {
		let host = NSHostingView(rootView: view.tint(Color.accent).background(.surfaceWindow))
		host.frame.size = Self.size
		let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
		window.appearance = NSAppearance(named: .aqua)
		window.contentView = host
		host.layoutSubtreeIfNeeded()
		RunLoop.main.run(until: .now.addingTimeInterval(settle))
		host.layoutSubtreeIfNeeded()
		let bitmap = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
		host.cacheDisplay(in: host.bounds, to: bitmap)
		let image = NSImage(size: Self.size)
		image.addRepresentation(bitmap)
		return image
	}

	/// One frame per sampled time, left to right, at a third of full size.
	private func strip(named name: String, from: NSImage, to: NSImage, transition: MotionTransition) throws {
		let scale = 1.0 / 3
		let frame = NSSize(width: Self.size.width * scale, height: Self.size.height * scale)
		let gap = 8.0, caption = 18.0
		let total = NSSize(width: (frame.width + gap) * Double(Self.times.count) - gap, height: frame.height + caption)
		let image = NSImage(size: total, flipped: false) { _ in
			NSColor.white.setFill()
			NSRect(origin: .zero, size: total).fill()
			for (index, time) in Self.times.enumerated() {
				let origin = NSPoint(x: Double(index) * (frame.width + gap), y: caption)
				let box = NSRect(origin: origin, size: frame)
				NSGraphicsContext.saveGraphicsState()
				NSBezierPath(rect: box).addClip()
				Self.draw(from, in: box, side: transition.removal, progress: transition.removal.motion.progress(at: time), leaving: true, scale: scale)
				Self.draw(to, in: box, side: transition.insertion, progress: transition.insertion.motion.progress(at: time), leaving: false, scale: scale)
				NSGraphicsContext.restoreGraphicsState()
				NSColor(white: 0.8, alpha: 1).setStroke()
				NSBezierPath(rect: box).stroke()
				Self.label("\(Int((time * 1000).rounded())) ms", at: NSPoint(x: origin.x, y: 2))
			}
			return true
		}
		try write(image, named: name)
	}

	private static func draw(_ image: NSImage, in box: NSRect, side: MotionTransition.Side, progress: Double, leaving: Bool, scale: Double) {
		let shown = leaving ? 1 - progress : progress
		let travel = leaving ? progress : 1 - progress
		let alpha = side.fades ? shown : (leaving && progress >= 1 ? 0 : 1)
		let offset = NSPoint(x: side.offset.width * travel * scale, y: -side.offset.height * travel * scale)
		image.draw(in: box.offsetBy(dx: offset.x, dy: offset.y), from: .zero, operation: .sourceOver, fraction: alpha)
	}

	private static func label(_ text: String, at point: NSPoint, color: NSColor = .darkGray) {
		NSAttributedString(string: text, attributes: [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: color]).draw(at: point)
	}

	private func write(_ image: NSImage, named name: String) throws {
		try FileManager.default.createDirectory(at: Self.output, withIntermediateDirectories: true)
		let tiff = try #require(image.tiffRepresentation)
		let png = try #require(NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]))
		try png.write(to: Self.output.appending(path: "\(name).png"))
	}
}
