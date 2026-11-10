import { createHash } from 'node:crypto';

export interface Rng {
	next: () => number;
	int: (min: number, max: number) => number;
	pick: <T>(items: readonly T[]) => T;
	chance: (probability: number) => boolean;
	fill: (bytes: Uint8Array) => Uint8Array;
}

export const createRng = (seed: number): Rng => {
	let state = seed >>> 0;
	const next = () => {
		state = (state + 0x6d2b79f5) >>> 0;
		let t = state;
		t = Math.imul(t ^ (t >>> 15), t | 1);
		t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
		return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
	};
	const int = (min: number, max: number) => min + Math.floor(next() * (max - min + 1));
	return {
		next,
		int,
		pick: items => items[int(0, items.length - 1)],
		chance: probability => next() < probability,
		fill: bytes => {
			for (let i = 0; i < bytes.length; i++) {
				bytes[i] = int(0, 255);
			}
			return bytes;
		}
	};
};

export const formatUuidV4 = (bytes: Uint8Array): string => {
	const b = Uint8Array.from(bytes.subarray(0, 16));
	b[6] = (b[6] & 0x0f) | 0x40;
	b[8] = (b[8] & 0x3f) | 0x80;
	const hex = Buffer.from(b).toString('hex');
	return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
};

export const nameId = (name: string): string =>
	formatUuidV4(createHash('sha256').update(name).digest());
