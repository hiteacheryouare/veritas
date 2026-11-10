import Testing
import VeritasCore

@Test func theCoreAnswersAcrossTheBridge() throws {
	#expect(try coreVersion() == "0.1.0")
}

@Test func aValidMonthComesBackAsARecord() throws {
	#expect(try parseMonth(text: "2027-03") == Month(year: 2027, month: 3))
}

@Test func anInvalidMonthThrowsTheCoreError() {
	#expect {
		try parseMonth(text: "2027-13")
	} throws: { error in
		guard case FfiError.Invalid(let message) = error else { return false }
		return message.contains("2027-13")
	}
}
