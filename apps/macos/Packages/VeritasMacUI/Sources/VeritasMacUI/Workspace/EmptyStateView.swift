import SwiftUI
import VeritasDesign

/// First launch, or after Close Budget: the splash, with two equal ways in
/// (D53), so neither is the default button. The coin mints in on the first
/// splash of a launch; later ones, and screenshots, are already still.
struct EmptyStateView: View {
	let isWorking: Bool
	let newBudget: () -> Void
	let importActualZip: () -> Void
	var playsMintIn = !EmptyStateView.hasPlayedMintIn

	@MainActor static var hasPlayedMintIn = false

	@State private var progress = 1.0
	@State private var isShown = true
	@Environment(\.accessibilityReduceMotion) private var reduceMotion

	var body: some View {
		SplashContent(progress: reduceMotion ? 1 : progress, isWorking: isWorking, newBudget: newBudget, importActualZip: importActualZip)
			.opacity(isShown ? 1 : 0)
			.motion(.mintIn, value: progress)
			.motion(.mintIn, value: isShown)
			.frame(maxWidth: .infinity, maxHeight: .infinity)
			.background {
				LinearGradient(colors: [.brand, .brandDeep], startPoint: .top, endPoint: .bottom)
					.ignoresSafeArea()
			}
			.environment(\.colorScheme, .dark)
			.navigationTitle("Veritas")
			.toolbar(removing: .title)
			.onAppear(perform: mintIn)
	}

	private func mintIn() {
		guard playsMintIn else { return }
		Self.hasPlayedMintIn = true
		progress = 0
		isShown = !reduceMotion
		Task { @MainActor in
			progress = 1
			isShown = true
		}
	}
}

/// The splash at one point of the mint-in.
struct SplashContent: View, Animatable {
	var progress: Double
	let isWorking: Bool
	let newBudget: () -> Void
	let importActualZip: () -> Void

	nonisolated var animatableData: Double {
		get { progress }
		set { progress = newValue }
	}

	var body: some View {
		let actions = MintIn.stage(progress, MintIn.actions)
		VStack(spacing: Spacing.s3) {
			MintInLockup(progress: progress, capHeight: 36)
				.foregroundStyle(.textOnBrand)
				.padding(.bottom, Spacing.s5)
			VStack(spacing: Spacing.s3) {
				Text("No budget open")
					.font(.text(.title, weight: .semibold))
					.foregroundStyle(.textOnBrand)
				Text("Start a new budget, or import the .zip from Export data in Actual's settings.")
					.font(.text(.body))
					.foregroundStyle(.textOnBrandSecondary)
					.multilineTextAlignment(.center)
				HStack(spacing: Spacing.s3) {
					Button("New budget…", action: newBudget)
						.accessibilityIdentifier("newBudget")
					Button("Import from Actual…", action: importActualZip)
						.accessibilityIdentifier("importFromActual")
				}
				.buttonStyle(SplashButtonStyle())
				.disabled(isWorking)
				.padding(.top, Spacing.s2)
				ProgressView()
					.controlSize(.small)
					.opacity(isWorking ? 1 : 0)
					.accessibilityHidden(!isWorking)
			}
			.offset(y: (1 - actions) * Spacing.s3)
			.opacity(actions)
		}
		.padding(Spacing.s6)
	}
}

/// A light-on-brand capsule. Both ways in look alike, so neither reads as the default.
/// It draws its own focus ring, a brighter border. The border is a filled ring:
/// a stroked one left hairlines at the capsule's ends.
struct SplashButtonStyle: ButtonStyle {
	static let fill = 0.14
	static let pressedFill = 0.24
	static let shape = RoundedRectangle(cornerRadius: Size.control / 2, style: .continuous)
	@Environment(\.isEnabled) private var isEnabled
	@Environment(\.isFocused) private var isFocused

	func makeBody(configuration: Configuration) -> some View {
		configuration.label
			.font(.text(.body, weight: .medium))
			.foregroundStyle(.textOnBrand)
			.padding(.horizontal, Spacing.s4)
			.frame(height: Size.control)
			.background(Color.white.opacity(configuration.isPressed ? Self.pressedFill : Self.fill), in: Self.shape)
			.overlay {
				let width: CGFloat = isFocused ? 2 : 1
				Self.shape.subtracting(Self.shape.inset(by: width)).fill(Color.white.opacity(isFocused ? 0.9 : 0.28))
			}
			.contentShape(Self.shape)
			.focusEffectDisabled()
			.opacity(isEnabled ? 1 : 0.55)
	}
}
