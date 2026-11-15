import SwiftUI

extension View {
	/// An identifier set on a plain container is stamped onto every element
	/// inside it, replacing theirs, so the container becomes its own element first.
	func containerIdentifier(_ identifier: String) -> some View {
		accessibilityElement(children: .contain)
			.accessibilityIdentifier(identifier)
	}
}
