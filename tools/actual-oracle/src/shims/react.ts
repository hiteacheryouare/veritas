// chart-theme.ts imports React hooks for its components; the oracle only
// calls its plain functions, so the hooks never run.
const unused = () => {
	throw new Error('The oracle runs no React components');
};
export const useMemo = unused;
export const useSyncExternalStore = unused;

export default { useMemo, useSyncExternalStore };
