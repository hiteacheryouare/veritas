// Stands in for loot-core's client connection when the oracle runs Actual's
// report code: every `send` goes to the Actual session the oracle opened.
type Send = (name: string, args?: unknown) => Promise<any>;

let target: Send | null = null;

export const connectTo = (send: Send) => {
	target = send;
};

export const send: Send = (name, args) => {
	if (!target) {
		throw new Error(`Actual's report code sent "${name}" before the oracle connected a session`);
	}
	return target(name, args);
};
