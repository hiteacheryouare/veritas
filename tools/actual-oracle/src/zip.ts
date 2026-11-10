import { inflateRawSync } from 'node:zlib';

const EOCD_SIGNATURE = 0x06054b50;
const CENTRAL_SIGNATURE = 0x02014b50;
const LOCAL_SIGNATURE = 0x04034b50;

const findEndOfCentralDirectory = (buf: Buffer) => {
	const earliest = Math.max(0, buf.length - 22 - 0xffff);
	for (let i = buf.length - 22; i >= earliest; i--) {
		if (buf.readUInt32LE(i) === EOCD_SIGNATURE) {
			return i;
		}
	}
	throw new Error('Not a zip file: end of central directory not found');
};

export const readZip = (data: Uint8Array): Map<string, Uint8Array> => {
	const buf = Buffer.from(data.buffer, data.byteOffset, data.byteLength);
	const eocd = findEndOfCentralDirectory(buf);
	const count = buf.readUInt16LE(eocd + 10);
	let offset = buf.readUInt32LE(eocd + 16);
	const entries = new Map<string, Uint8Array>();
	for (let i = 0; i < count; i++) {
		if (buf.readUInt32LE(offset) !== CENTRAL_SIGNATURE) {
			throw new Error('Corrupt zip: bad central directory entry');
		}
		const method = buf.readUInt16LE(offset + 10);
		const compressedSize = buf.readUInt32LE(offset + 20);
		const nameLength = buf.readUInt16LE(offset + 28);
		const extraLength = buf.readUInt16LE(offset + 30);
		const commentLength = buf.readUInt16LE(offset + 32);
		const localOffset = buf.readUInt32LE(offset + 42);
		const name = buf.toString('utf8', offset + 46, offset + 46 + nameLength);
		offset += 46 + nameLength + extraLength + commentLength;

		if (buf.readUInt32LE(localOffset) !== LOCAL_SIGNATURE) {
			throw new Error(`Corrupt zip: bad local header for ${name}`);
		}
		const dataStart = localOffset + 30 + buf.readUInt16LE(localOffset + 26) + buf.readUInt16LE(localOffset + 28);
		const compressed = buf.subarray(dataStart, dataStart + compressedSize);
		if (method === 0) {
			entries.set(name, compressed);
		} else if (method === 8) {
			entries.set(name, inflateRawSync(compressed));
		} else {
			throw new Error(`Unsupported zip compression method ${method} for ${name}`);
		}
	}
	return entries;
};

export interface BudgetFiles {
	db: Uint8Array;
	metadata: Uint8Array;
}

export const locateBudgetFiles = (entries: Map<string, Uint8Array>): BudgetFiles => {
	const names = [...entries.keys()];
	const dirsOf = (file: string) =>
		names.filter(name => name === file || name.endsWith(`/${file}`)).map(name => name.slice(0, -file.length));
	const metaDirs = dirsOf('metadata.json');
	const shared = dirsOf('db.sqlite').filter(dir => metaDirs.includes(dir));
	const dir = shared.includes('') ? '' : shared.length === 1 ? shared[0] : null;
	if (dir === null) {
		throw new Error('Zip has no db.sqlite and metadata.json pair at its root or in exactly one shared folder');
	}
	return {
		db: entries.get(`${dir}db.sqlite`) as Uint8Array,
		metadata: entries.get(`${dir}metadata.json`) as Uint8Array
	};
};
