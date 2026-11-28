import AppKit
import SwiftUI
import Testing
import VeritasDesign
@testable import VeritasMacUI

/// The places the logo appears, light and dark, into target/screenshots/brand.
@MainActor
struct BrandScreenshotTests {
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/brand")
		.standardizedFileURL

	@Test func firstLaunch() throws {
		try render(EmptyStateView(isWorking: false, newBudget: {}, importActualZip: {}).frame(width: 760, height: 460), named: "first-launch")
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
