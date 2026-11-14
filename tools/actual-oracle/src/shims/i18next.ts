// Report modules translate their labels at import time; the oracle only reads
// numbers, so a label stays its English key.
export const t = (key: string) => key;

export default { t };
