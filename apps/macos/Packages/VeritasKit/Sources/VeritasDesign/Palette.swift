import SwiftUI

public extension ColorToken {
	static let surfaceWindow = ColorToken(light: RGBA(0xf3f3f6), dark: RGBA(0x1e1e22))
	static let surfaceSidebar = ColorToken(light: RGBA(0xeceef3, alpha: 0.78), dark: RGBA(0x2c2d34, alpha: 0.72))
	static let surfaceGrouped = ColorToken(light: RGBA(0xfcfcfd), dark: RGBA(0x27272c))
	static let surfaceRaised = ColorToken(light: RGBA(0xfbfbfc), dark: RGBA(0x2e2e34))
	static let surfaceField = ColorToken(light: RGBA(0xfdfdfe), dark: RGBA(0x1a1a1e))
	static let surfacePaper = ColorToken(light: RGBA(0xfcfcfa), dark: RGBA(0xf4f4f1))
	static let surfaceScrim = ColorToken(light: RGBA(0x1c1e26, alpha: 0.22), dark: RGBA(0x06060c, alpha: 0.45))
	static let surfaceHover = ColorToken(light: RGBA(0x14161e, alpha: 0.04), dark: RGBA(0xf0f0fa, alpha: 0.05))
	static let surfaceControl = ColorToken(light: RGBA(0xfafafc, alpha: 0.85), dark: RGBA(0x3c3d44, alpha: 0.8))
	static let edgeWindow = ColorToken(light: RGBA(0x14161e, alpha: 0.12), dark: RGBA(0xf0f0fa, alpha: 0.12))
	static let edgeSidebar = ColorToken(light: RGBA(0xfafafc, alpha: 0.7), dark: RGBA(0xf0f0fa, alpha: 0.08))
	static let edgeGrouped = ColorToken(light: RGBA(0x14161e, alpha: 0.07), dark: RGBA(0xf0f0fa, alpha: 0.06))
	static let edgeControl = ColorToken(light: RGBA(0x14161e, alpha: 0.1), dark: RGBA(0xf0f0fa, alpha: 0.1))
	static let separator = ColorToken(light: RGBA(0x14161e, alpha: 0.08), dark: RGBA(0xf0f0fa, alpha: 0.08))
	static let textPrimary = ColorToken(light: RGBA(0x1c1c21), dark: RGBA(0xececf0))
	static let textSecondary = ColorToken(light: RGBA(0x5d5d66), dark: RGBA(0xb0b0b9))
	static let textTertiary = ColorToken(light: RGBA(0x5f5f68), dark: RGBA(0xafafb7))
	static let textPaper = ColorToken(light: RGBA(0x23232a), dark: RGBA(0x23232a))
	static let accent = ColorToken(light: RGBA(0x23169c), dark: RGBA(0x98acff))
	static let accentFill = ColorToken(light: RGBA(0x23169c), dark: RGBA(0x98acff))
	static let accentSoft = ColorToken(light: RGBA(0x23169c, alpha: 0.1), dark: RGBA(0x98acff, alpha: 0.14))
	static let surfaceSelected = ColorToken(light: RGBA(0x14161e, alpha: 0.08), dark: RGBA(0xf0f0fa, alpha: 0.12))
	static let focusRing = ColorToken(light: RGBA(0x23169c), dark: RGBA(0x98acff))
	static let textOnAccent = ColorToken(light: RGBA(0xf5f5f7), dark: RGBA(0x1c1c21))
	static let brandSecondary = ColorToken(light: RGBA(0x00688b), dark: RGBA(0x59bbe6))
	static let brandSecondaryFill = ColorToken(light: RGBA(0x007ea7), dark: RGBA(0x007ea7))
	static let brand = ColorToken(light: RGBA(0x23169c), dark: RGBA(0x23169c))
	static let textOnBrand = ColorToken(light: RGBA(0xf5f5f7), dark: RGBA(0xf5f5f7))
	static let track = ColorToken(light: RGBA(0x14161e, alpha: 0.08), dark: RGBA(0xf0f0fa, alpha: 0.09))
	static let moneyAvailable = ColorToken(light: RGBA(0x036f46), dark: RGBA(0x56e39f))
	static let moneyAvailableFill = ColorToken(light: RGBA(0x56e39f), dark: RGBA(0x56e39f))
	static let moneyLow = ColorToken(light: RGBA(0x7c5b02), dark: RGBA(0xf5b700))
	static let moneyLowFill = ColorToken(light: RGBA(0xf5b700), dark: RGBA(0xf5b700))
	static let moneyOverspent = ColorToken(light: RGBA(0xc30225), dark: RGBA(0xff908a))
	static let moneyOverspentFill = ColorToken(light: RGBA(0xdb162f), dark: RGBA(0xdb162f))
	static let moneyIncome = ColorToken(light: RGBA(0x00688b), dark: RGBA(0x59bbe6))
	static let moneyIncomeFill = ColorToken(light: RGBA(0x007ea7), dark: RGBA(0x007ea7))
	static let moneyNeutral = ColorToken(light: RGBA(0x5f5f68), dark: RGBA(0xafafb7))
	static let chartExpense = ColorToken(light: RGBA(0x6e6e78), dark: RGBA(0xa5a5ae))
	static let chartExpenseSoft = ColorToken(light: RGBA(0x6e6e78, alpha: 0.28), dark: RGBA(0xa5a5ae, alpha: 0.3))
	static let chartSavings = ColorToken(light: RGBA(0x56e39f), dark: RGBA(0x56e39f))
	static let chartDebtPaydown = ColorToken(light: RGBA(0x8b5e34), dark: RGBA(0xc99a6b))
	static let chartUnallocated = ColorToken(light: RGBA(0x007ea7, alpha: 0.35), dark: RGBA(0x007ea7, alpha: 0.4))
	static let chartBand = ColorToken(light: RGBA(0x3a3a42, alpha: 0.2), dark: RGBA(0xe2e2e8, alpha: 0.2))
	static let chartGrid = ColorToken(light: RGBA(0x14161e, alpha: 0.07), dark: RGBA(0xf0f0fa, alpha: 0.07))
	static let scenarioBaseline = ColorToken(light: RGBA(0x3a3a42), dark: RGBA(0xe2e2e8))
	static let scenario1 = ColorToken(light: RGBA(0x2f84dc), dark: RGBA(0x5eaaf5))
	static let scenario2 = ColorToken(light: RGBA(0x8b5e34), dark: RGBA(0xc99a6b))
	static let scenario3 = ColorToken(light: RGBA(0x0f8a9a), dark: RGBA(0x4cc0cf))
}

public extension ShapeStyle where Self == Color {
	static var surfaceWindow: Color { Color(ColorToken.surfaceWindow) }
	static var surfaceSidebar: Color { Color(ColorToken.surfaceSidebar) }
	static var surfaceGrouped: Color { Color(ColorToken.surfaceGrouped) }
	static var surfaceRaised: Color { Color(ColorToken.surfaceRaised) }
	static var surfaceField: Color { Color(ColorToken.surfaceField) }
	static var surfacePaper: Color { Color(ColorToken.surfacePaper) }
	static var surfaceScrim: Color { Color(ColorToken.surfaceScrim) }
	static var surfaceHover: Color { Color(ColorToken.surfaceHover) }
	static var surfaceControl: Color { Color(ColorToken.surfaceControl) }
	static var edgeWindow: Color { Color(ColorToken.edgeWindow) }
	static var edgeSidebar: Color { Color(ColorToken.edgeSidebar) }
	static var edgeGrouped: Color { Color(ColorToken.edgeGrouped) }
	static var edgeControl: Color { Color(ColorToken.edgeControl) }
	static var separator: Color { Color(ColorToken.separator) }
	static var textPrimary: Color { Color(ColorToken.textPrimary) }
	static var textSecondary: Color { Color(ColorToken.textSecondary) }
	static var textTertiary: Color { Color(ColorToken.textTertiary) }
	static var textPaper: Color { Color(ColorToken.textPaper) }
	static var accent: Color { Color(ColorToken.accent) }
	static var accentFill: Color { Color(ColorToken.accentFill) }
	static var accentSoft: Color { Color(ColorToken.accentSoft) }
	static var surfaceSelected: Color { Color(ColorToken.surfaceSelected) }
	static var focusRing: Color { Color(ColorToken.focusRing) }
	static var textOnAccent: Color { Color(ColorToken.textOnAccent) }
	static var brandSecondary: Color { Color(ColorToken.brandSecondary) }
	static var brandSecondaryFill: Color { Color(ColorToken.brandSecondaryFill) }
	static var brand: Color { Color(ColorToken.brand) }
	static var textOnBrand: Color { Color(ColorToken.textOnBrand) }
	static var track: Color { Color(ColorToken.track) }
	static var moneyAvailable: Color { Color(ColorToken.moneyAvailable) }
	static var moneyAvailableFill: Color { Color(ColorToken.moneyAvailableFill) }
	static var moneyLow: Color { Color(ColorToken.moneyLow) }
	static var moneyLowFill: Color { Color(ColorToken.moneyLowFill) }
	static var moneyOverspent: Color { Color(ColorToken.moneyOverspent) }
	static var moneyOverspentFill: Color { Color(ColorToken.moneyOverspentFill) }
	static var moneyIncome: Color { Color(ColorToken.moneyIncome) }
	static var moneyIncomeFill: Color { Color(ColorToken.moneyIncomeFill) }
	static var moneyNeutral: Color { Color(ColorToken.moneyNeutral) }
	static var chartExpense: Color { Color(ColorToken.chartExpense) }
	static var chartExpenseSoft: Color { Color(ColorToken.chartExpenseSoft) }
	static var chartSavings: Color { Color(ColorToken.chartSavings) }
	static var chartDebtPaydown: Color { Color(ColorToken.chartDebtPaydown) }
	static var chartUnallocated: Color { Color(ColorToken.chartUnallocated) }
	static var chartBand: Color { Color(ColorToken.chartBand) }
	static var chartGrid: Color { Color(ColorToken.chartGrid) }
	static var scenarioBaseline: Color { Color(ColorToken.scenarioBaseline) }
	static var scenario1: Color { Color(ColorToken.scenario1) }
	static var scenario2: Color { Color(ColorToken.scenario2) }
	static var scenario3: Color { Color(ColorToken.scenario3) }
}

extension ColorToken {
	static let byName: [String: ColorToken] = [
		"surface-window": .surfaceWindow,
		"surface-sidebar": .surfaceSidebar,
		"surface-grouped": .surfaceGrouped,
		"surface-raised": .surfaceRaised,
		"surface-field": .surfaceField,
		"surface-paper": .surfacePaper,
		"surface-scrim": .surfaceScrim,
		"surface-hover": .surfaceHover,
		"surface-control": .surfaceControl,
		"edge-window": .edgeWindow,
		"edge-sidebar": .edgeSidebar,
		"edge-grouped": .edgeGrouped,
		"edge-control": .edgeControl,
		"separator": .separator,
		"text-primary": .textPrimary,
		"text-secondary": .textSecondary,
		"text-tertiary": .textTertiary,
		"text-paper": .textPaper,
		"accent": .accent,
		"accent-fill": .accentFill,
		"accent-soft": .accentSoft,
		"surface-selected": .surfaceSelected,
		"focus-ring": .focusRing,
		"text-on-accent": .textOnAccent,
		"brand-secondary": .brandSecondary,
		"brand-secondary-fill": .brandSecondaryFill,
		"brand": .brand,
		"text-on-brand": .textOnBrand,
		"track": .track,
		"money-available": .moneyAvailable,
		"money-available-fill": .moneyAvailableFill,
		"money-low": .moneyLow,
		"money-low-fill": .moneyLowFill,
		"money-overspent": .moneyOverspent,
		"money-overspent-fill": .moneyOverspentFill,
		"money-income": .moneyIncome,
		"money-income-fill": .moneyIncomeFill,
		"money-neutral": .moneyNeutral,
		"chart-expense": .chartExpense,
		"chart-expense-soft": .chartExpenseSoft,
		"chart-savings": .chartSavings,
		"chart-debt-paydown": .chartDebtPaydown,
		"chart-unallocated": .chartUnallocated,
		"chart-band": .chartBand,
		"chart-grid": .chartGrid,
		"scenario-baseline": .scenarioBaseline,
		"scenario-1": .scenario1,
		"scenario-2": .scenario2,
		"scenario-3": .scenario3
	]
}
