import Testing
@testable import VeritasKit

/// v18: how the shell tells a stored goal icon's two kinds apart.
struct GoalIconTests {
	@Test func allASCIIIsASymbolNameAndAnythingElseAnEmoji() {
		#expect(GoalIcon("laptopcomputer") == .symbol("laptopcomputer"))
		#expect(GoalIcon("car.fill") == .symbol("car.fill"))
		#expect(GoalIcon("🗾") == .emoji("🗾"))
		#expect(GoalIcon("❤️") == .emoji("❤️"))
		#expect(GoalIcon("1️⃣") == .emoji("1️⃣"), "a keycap starts with an ASCII digit but isn't all ASCII")
		#expect(GoalIcon(nil) == nil)
		#expect(GoalIcon("") == nil)
		#expect(GoalIcon("🗾")?.stored == "🗾")
	}

	@Test func thePickersSymbolsAreNamesTheCoreAccepts() {
		// An SF Symbol name is a-z, 0-9 and ".", 1 to 64 characters, not starting or ending with ".".
		#expect(Set(GoalIcon.symbols).count == GoalIcon.symbols.count)
		for name in GoalIcon.symbols {
			#expect(!name.isEmpty && name.utf8.count <= 64 && !name.hasPrefix(".") && !name.hasSuffix("."), "\(name)")
			#expect(name.unicodeScalars.allSatisfy { ("a"..."z").contains($0) || ("0"..."9").contains($0) || $0 == "." }, "\(name)")
		}
	}
}
