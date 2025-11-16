const MIN_WINDOW_OPACITY = 0.6;
const MAX_WINDOW_OPACITY = 1;

export const clampWindowOpacity = (value: number): number => {
    if (Number.isNaN(value)) {
        return MIN_WINDOW_OPACITY;
    }
    return Math.min(MAX_WINDOW_OPACITY, Math.max(MIN_WINDOW_OPACITY, value));
};

export const applyWindowOpacityVariable = (value: number): void => {
    const clamped = clampWindowOpacity(value);
    document.documentElement.style.setProperty("--window-opacity", clamped.toFixed(2));
};
