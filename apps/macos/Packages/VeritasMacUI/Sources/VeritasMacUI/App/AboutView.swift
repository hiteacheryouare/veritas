import AppKit
import SwiftUI
import VeritasDesign

/// Veritas › About Veritas: the app icon, the wordmark, the version and the
/// licences the app's notices list.
struct AboutView: View {
	static let windowID = "about"

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
			Text("Includes code ported from Actual Budget and a coin from Bootstrap Icons, both MIT licensed.")
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
