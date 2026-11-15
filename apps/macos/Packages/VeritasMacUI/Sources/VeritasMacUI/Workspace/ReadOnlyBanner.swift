import SwiftUI
import VeritasKit

struct ReadOnlyBanner: View {
	let notice: ReadOnlyNotice
	let perform: (ReadOnlyNotice.Action) -> Void

	var body: some View {
		NoticeBanner(symbol: "lock", message: notice.message, actionTitle: notice.action?.title) {
			if let action = notice.action { perform(action) }
		}
		.accessibilityIdentifier("readOnlyBanner")
	}
}
