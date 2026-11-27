import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// The places the logo appears, light and dark, into target/screenshots/brand.
@MainActor
struct BrandScreenshotTests {
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/brand")
		.standardizedFileURL

	@Test func firstLaunch() throws {
		try render(EmptyStateView(isWorking: false, newBudget: {}, importActualZip: {}, playsMintIn: false).frame(width: 760, height: 460), named: "first-launch")
	}

	@Test func splashMintInFrames() throws {
		for progress in [0.15, 0.35, 0.55, 0.8, 1] {
			let frame = SplashContent(progress: progress, isWorking: false, newBudget: {}, importActualZip: {})
				.frame(width: 760, height: 460)
				.background(LinearGradient(colors: [.brand, .brandDeep], startPoint: .top, endPoint: .bottom))
				.environment(\.colorScheme, .dark)
			try render(frame, named: "mint-in-\(Int(progress * 100))")
		}
	}

	@Test func theMintInEndsOnTheStillLockup() {
		#expect(MintIn.stage(1, MintIn.rings) == 1)
		#expect(MintIn.stage(1, MintIn.actions) == 1)
		#expect(MintIn.stage(0, MintIn.rings) == 0)
		#expect(MintIn.stage(0.5, MintIn.actions) == 0, "the actions wait for the lockup")
		#expect(Motion.mintIn.duration == 1.2)
		#expect(Motion.mintIn.reduced != nil && Motion.mintIn.duration(reduceMotion: true) <= Motion.Duration.fast, "Reduce Motion gives a short fade")
	}

	@Test func accountGraphCrossingZero() throws {
		let balances: [Int64] = [42_000, 30_000, 12_000, -8_000, -21_000, -6_000, 9_000, 18_000]
		let points = balances.enumerated().map { BalancePoint(date: String(format: "2026-11-%02d", $0.offset + 1), balance: $0.element) }
		let graph = BalanceGraph(history: BalanceHistory(points: points, change: -0.57), format: MoneyFormat())
			.frame(width: 760, height: 120)
			.padding(Spacing.s4)
		try render(graph, named: "account-graph-crossing-zero")
	}

	@Test func aboutShowsLicensesCopyright() throws {
		let license = try String(contentsOf: Self.output.appending(path: "../../../LICENSE").standardizedFileURL, encoding: .utf8)
		#expect(license.contains(AboutView.copyright))
	}

	@Test func about() throws {
		let icon = NSImage(contentsOf: Self.output.appending(path: "../../../design/brand/app-icon/png/icon_128x128@2x.png").standardizedFileURL) ?? NSImage()
		try render(AboutView(icon: icon, version: "0.1.0", build: "1"), named: "about")
	}

	@Test func lockupSizes() throws {
		let sizes: [CGFloat] = [11, 16, 24, 40]
		let view = VStack(alignment: .leading, spacing: 24) {
			ForEach(sizes, id: \.self) { VeritasLockup(capHeight: $0).foregroundStyle(.accent) }
		}
		.padding(32)
		try render(view, named: "lockup-sizes")
	}

	@Test func theLockupScalesWithItsCapHeight() throws {
		let small = NSHostingView(rootView: VeritasLockup(capHeight: 10)).fittingSize
		let large = NSHostingView(rootView: VeritasLockup(capHeight: 40)).fittingSize
		#expect(abs(large.width / small.width - 4) < 0.05, "\(small) \(large)")
		#expect(large.height >= 40 * 1.08, "the coin is taller than the caps")
	}

	private func render(_ view: some View, named name: String) throws {
		try FileManager.default.createDirectory(at: Self.output, withIntermediateDirectories: true)
		for (appearance, suffix) in [(NSAppearance.Name.aqua, "light"), (.darkAqua, "dark")] {
			let host = NSHostingView(rootView: view.tint(Color.accent).background(.surfaceWindow))
			host.frame.size = host.fittingSize
			let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
			window.appearance = NSAppearance(named: appearance)
			window.contentView = host
			host.layoutSubtreeIfNeeded()
			let bitmap = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
			host.cacheDisplay(in: host.bounds, to: bitmap)
			let png = try #require(bitmap.representation(using: .png, properties: [:]))
			try png.write(to: Self.output.appending(path: "\(name)-\(suffix).png"))
		}
	}
}
