import type { ActualArithmetic } from './reference.ts';

// Inputs for the engine's arithmetic vectors
// (crates/veritas-engine/tests/fixtures/actual-arithmetic-vectors.json).
// Every result comes from Actual's own code; the Rust tests compare bit for bit.
export const FORMATS = ['comma-dot', 'dot-comma', 'space-comma', 'apostrophe-dot', 'comma-dot-in'];

const EXPRESSIONS = [
	'', ' ', '0', '-0', '5', '-5', '+5', '--5', '-(5)', '- 5', '.', '.5', '5.', '-.5',
	'1/0', '-1/0', '0/0', '1/-0', '100.00/0', '(1/0)-(1/0)', '1^(1/0)', '(0-1)^(1/0)', '2^1024', '(0-1)^0.5',
	'0^0', '2^-1', '-2^2', '2^3^2', '(2^3)^2', '2^2^-1', '1.1^2', '2^0.5', '10^-2', '1.05^12', '1.0025^360',
	'1,234.56+10', '1.234,56+10', '1 234,56+10', '1\u202F234,56+10', '1\u00A0234,56', '1\u2019234.56+10', '12,34,567.89',
	'(5', '5)', '((5)', '(5))', '()', '(', ')', '5%', '1e5', '1E5', '1+2)foo', '10 - -4', '10+-4', '10*-4', '10/-4',
	'\u22125', '5\u22123', '1.2.3', '1,23', '1,234', '1,2345', '1.234', '1.2345', '1,234,567', '1.234.567', '1,234.567',
	'$1,234.56', '-$5', '$-5', '$5', '5$', '$$5', '1$2', '$', '\u20AC12,50', '\u20AC12.50', '\u00A35', '\u00A5100', '\u20B91,00,000',
	'\u20C15', '\u{11FDD}5', '\u{11FDD}1,234', '12\u{1E2FF},345', '1\u{11FDD},23', 'USD 5', '5 USD', 'CHF1\u2019000.50',
	'100.00+12.34', '100.00-12.34', '100.00*2', '100.00/3', '100.00^2', '100.00+', '100.00-', '100.00*', '100.00^',
	'1,234.56+', '0.00+12.34', '-50.00+12.34', '0.1+0.2', '1.005', '12.345', '0.125', '0.5', '-0.5', '2.675', '1.015',
	'1,234,567.891', '9007199254740993', '22517998136852.47', '22517998136852.48', '-22517998136852.47', '22517998136852.475',
	"12'345.67", "1'000'000.50", '1 2 3', '1\t2', '1\n+2', '\u20005', '5\uFEFF', '1,,2', '1..2', ',5', ',', ',,',
	'(12 + 3) + (10)', '20^3 - 5 * (10 / 2)', '24 / 3 * 2', '2400 / 2 / 5', '(((((((1+1)))))))', '3*(2+(4-1))^2',
	'1--1', '1---1', '1+-+1', '*5', '/5', '^5', '5**2', '5//2', '5^^2', '1+2(3', '(1+2)x', '10+20 trailing', '0x10', '1_000',
	'99999999999999999999999', '1' + '0'.repeat(400), '0.' + '0'.repeat(400) + '1', '123456789012345678901234567890.5'
];

const PARSE_INPUTS = [
	'1.5e3', '1e5', 'Infinity', '-Infinity', '+5', ' 5', '5abc', 'abc', '1.2.3', '12.3\u20194', '-', '--', '-.5', '.',
	'1,234', '1,23', '1.234', '1.23', '1,234.56', '1.234,56', '12\u2212', '\u22121,5', '5-3', '1,2,3', '1,234,5', 'a,bcd',
	'+12.34', '(5', '1/2', '1,2e3', '1.2e3', '1.2E-3', '1.2e', '1.2e+', '1.e3', '.e3', '0x1A', '\u00A01', '1,5\u00A0', '1.0000000000000002'
];

const ROUND_INPUTS = [
	0, 0.1, 0.49999999999999994, 0.5, 1.5, 2.5, -0.5, -1.5, -2.5, -0.49999999999999994, 4503599627370495.5,
	4503599627370497, 1.005, 112.34, 1.015, 2.675, -1.005, 100.49999999999999, -100.5, 1e15 + 0.5, 0.29 * 100
];

const CENTS = [
	0, 1, -1, 5, 40, -40, 50, -50, 99, 100, 150, -150, 1050, 11234, -3475, 99999, 123456, 1234567, -123456789,
	10000000000, 2251799813685247, -2251799813685247, 100000000000000
];

// Numbers travel as JavaScript's own shortest round-trip text, so no JSON
// float parser can shift them by an ulp.
const encode = (v: number | null) => (v === null ? null : Object.is(v, -0) ? '-0' : String(v));

export const arithmeticVectors = (actual: ActualArithmetic): string => {
	const vectors: Record<string, unknown> = {
		source: `Actual ${actual.pin.tag} (${actual.pin.commit}) packages/loot-core/src/shared/arithmetic.ts and util.ts, unmodified, under Node ${process.version}, ICU ${process.versions.icu}, Unicode ${process.versions.unicode}`,
		evalArithmetic: [],
		currencyToAmount: [],
		amountToInteger: [],
		forEdit: []
	};
	const push = (key: string, row: unknown) => (vectors[key] as unknown[]).push(row);
	for (const format of FORMATS) {
		actual.setNumberFormat({ format, hideFraction: false });
		for (const expression of EXPRESSIONS) {
			push('evalArithmetic', { format, expression, result: encode(actual.evalArithmetic(expression)) });
		}
		for (const input of PARSE_INPUTS) {
			push('currencyToAmount', { format, input, result: encode(actual.currencyToAmount(input)) });
		}
		for (const hideFraction of [false, true]) {
			const formatter = actual.getNumberFormat({ format, decimalPlaces: hideFraction ? 0 : 2 }).formatter;
			for (const value of CENTS) {
				push('forEdit', { format, hideFraction, cents: value, text: formatter.format(value / 100) });
			}
		}
	}
	actual.setNumberFormat({ format: 'comma-dot', hideFraction: false });
	for (const input of ROUND_INPUTS) {
		push('amountToInteger', { input: encode(input), result: encode(Math.round(input)), cents: encode(actual.amountToInteger(input)) });
	}
	const lines = Object.entries(vectors).map(([key, value]) =>
		Array.isArray(value)
			? `\t${JSON.stringify(key)}: [\n${value.map(row => `\t\t${JSON.stringify(row)}`).join(',\n')}\n\t]`
			: `\t${JSON.stringify(key)}: ${JSON.stringify(value)}`
	);
	return `{\n${lines.join(',\n')}\n}\n`;
};
