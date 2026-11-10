export interface SqlReal {
	readonly sqlReal: number;
}

export type CanonicalValue =
	| null
	| boolean
	| string
	| number
	| bigint
	| SqlReal
	| Uint8Array
	| CanonicalValue[]
	| { [key: string]: CanonicalValue };

export const sqlReal = (value: number): SqlReal => ({ sqlReal: value });

const isSqlReal = (value: unknown): value is SqlReal =>
	typeof value === 'object' && value !== null && 'sqlReal' in value && Object.keys(value).length === 1;

export const compareCodePoints = (a: string, b: string): number => {
	const length = Math.min(a.length, b.length);
	for (let i = 0; i < length; i++) {
		const x = a.codePointAt(i) as number;
		const y = b.codePointAt(i) as number;
		if (x !== y) {
			return x - y;
		}
		if (x > 0xffff) {
			i++;
		}
	}
	return a.length - b.length;
};

export const formatReal = (value: number): string => {
	const text = String(value);
	return /[.e]/.test(text) ? text : `${text}.0`;
};

const formatNumber = (value: number): string => {
	if (!Number.isFinite(value)) {
		throw new Error(`Cannot encode non-finite number ${value}`);
	}
	return Number.isInteger(value) ? String(value) : formatReal(value);
};

const sortedEntries = (object: { [key: string]: CanonicalValue }) =>
	Object.entries(object)
		.filter(([, value]) => value !== undefined)
		.sort(([a], [b]) => compareCodePoints(a, b));

const compact = (value: CanonicalValue): string => {
	if (value === null || typeof value === 'boolean') {
		return String(value);
	}
	if (typeof value === 'string') {
		return JSON.stringify(value);
	}
	if (typeof value === 'bigint') {
		return value.toString();
	}
	if (typeof value === 'number') {
		return formatNumber(value);
	}
	if (value instanceof Uint8Array) {
		return `{"$blob":${JSON.stringify(Buffer.from(value).toString('hex'))}}`;
	}
	if (isSqlReal(value)) {
		return Number.isFinite(value.sqlReal) ? formatReal(value.sqlReal) : `{"$real":"${value.sqlReal}"}`;
	}
	if (Array.isArray(value)) {
		return `[${value.map(compact).join(',')}]`;
	}
	return `{${sortedEntries(value).map(([key, entry]) => `${JSON.stringify(key)}:${compact(entry)}`).join(',')}}`;
};

const isContainer = (value: CanonicalValue): value is CanonicalValue[] | { [key: string]: CanonicalValue } =>
	typeof value === 'object' && value !== null && !(value instanceof Uint8Array) && !isSqlReal(value);

export const writeCanonical = (value: CanonicalValue, write: (chunk: string) => void, depth = 0): void => {
	if (!isContainer(value)) {
		write(compact(value));
		return;
	}
	const inner = '\t'.repeat(depth + 1);
	if (Array.isArray(value)) {
		if (value.length === 0) {
			write('[]');
			return;
		}
		write('[\n');
		value.forEach((element, index) => {
			write(`${inner}${compact(element)}${index < value.length - 1 ? ',' : ''}\n`);
		});
		write(`${'\t'.repeat(depth)}]`);
		return;
	}
	const entries = sortedEntries(value);
	if (entries.length === 0) {
		write('{}');
		return;
	}
	write('{\n');
	entries.forEach(([key, entry], index) => {
		write(`${inner}${JSON.stringify(key)}: `);
		writeCanonical(entry, write, depth + 1);
		write(index < entries.length - 1 ? ',\n' : '\n');
	});
	write(`${'\t'.repeat(depth)}}`);
};

export const toCanonical = (value: CanonicalValue): string => {
	const chunks: string[] = [];
	writeCanonical(value, chunk => chunks.push(chunk));
	chunks.push('\n');
	return chunks.join('');
};
