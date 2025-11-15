import type { FallbackVisual, SearchResult } from "../types";

const FALLBACK_ICON_LIBRARY: FallbackVisual[] = [
    {
        glyph: "🌀",
        background: "linear-gradient(135deg, #8093ff, #72e1ff)",
        color: "#ffffff",
    },
    {
        glyph: "✨",
        background: "linear-gradient(135deg, #ff9a9e, #fad0c4)",
        color: "#4b1f29",
    },
    {
        glyph: "🚀",
        background: "linear-gradient(135deg, #70f1ff, #6d88ff)",
        color: "#0b1c32",
    },
    {
        glyph: "📁",
        background: "linear-gradient(135deg, #f6d365, #fda085)",
        color: "#4b230d",
    },
    {
        glyph: "🔖",
        background: "linear-gradient(135deg, #8ec5fc, #e0c3fc)",
        color: "#2b1b33",
    },
    {
        glyph: "🌐",
        background: "linear-gradient(135deg, #84fab0, #8fd3f4)",
        color: "#083828",
    },
    {
        glyph: "⚡",
        background: "linear-gradient(135deg, #fddb92, #d1fdff)",
        color: "#402a04",
    },
    {
        glyph: "🔎",
        background: "linear-gradient(135deg, #c3cfe2, #c3cfe2)",
        color: "#1a2433",
    },
    {
        glyph: "💡",
        background: "linear-gradient(135deg, #ffd3a5, #fd6585)",
        color: "#3d1204",
    },
    {
        glyph: "🧭",
        background: "linear-gradient(135deg, #f5f7fa, #c3cfe2)",
        color: "#1c2230",
    },
];

export const pickFallbackIcon = (item: SearchResult): FallbackVisual => {
    const basis = item.id || item.title || item.subtitle || String(item.score);
    let hash = 0;
    for (let index = 0; index < basis.length; index += 1) {
        hash = (hash << 5) - hash + basis.charCodeAt(index);
        hash |= 0;
    }
    const normalized = Math.abs(hash);
    return FALLBACK_ICON_LIBRARY[normalized % FALLBACK_ICON_LIBRARY.length];
};
