import AppKit
import SwiftUI
import VeritasDesign

/// Veritas › About Veritas: the app icon, the wordmark, the version and the
/// copyright notice from LICENSE. The MIT notices the app owes ship in the
/// bundle as Acknowledgements.md, copied from THIRD_PARTY_NOTICES.md at build time.
struct AboutView: View {
	static let windowID = "about"
	static let copyright = "Copyright 2026 The Veritas Financial Technologies Corporation of Philadelphia (Veritas), Lattice & Company, Ryan Mullin, and contributors."

	var icon = NSApp?.applicationIconImage ?? NSImage()
	var version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
	var build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String

	private var versionLine: String? {
		guard let version else { return nil }
		return build.map { "Version \(version) (\($0))" } ?? "Version \(version)"
	}

	var body: some View {
		VStack(spacing: Spacing.s3) {
			Image(nsImage: icon)
				.resizable()
				.frame(width: 112, height: 112)
				.accessibilityHidden(true)
			Wordmark()
				.aspectRatio(Wordmark.aspectRatio, contentMode: .fit)
				.frame(height: 34)
				.foregroundStyle(.accent)
				.accessibilityLabel("Veritas")
				.accessibilityAddTraits(.isHeader)
			if let versionLine {
				Text(versionLine)
					.font(.text(.body))
					.foregroundStyle(.textSecondary)
					.textSelection(.enabled)
			}
			Text(Self.copyright)
				.font(.text(.caption))
				.foregroundStyle(.textTertiary)
				.multilineTextAlignment(.center)
				.fixedSize(horizontal: false, vertical: true)
				.padding(.top, Spacing.s2)
		}
		.padding(.horizontal, Spacing.s6)
		.padding(.top, Spacing.s5)
		.padding(.bottom, Spacing.s6)
		.frame(width: 340)
		.background(.surfaceWindow)
	}
}
