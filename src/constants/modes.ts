import type { ModeConfig, ModeId } from "../types";

export const MODE_CONFIGS: Record<ModeId, ModeConfig> = {
    all: {
        id: "all",
        label: "智能模式",
        description: "搜索应用与网页",
        placeholder: "搜索应用和网页（支持拼音/首字母）",
    },
    bookmark: {
        id: "bookmark",
        label: "书签模式",
        prefix: "b",
        description: "仅在收藏夹中查找",
        placeholder: "书签模式 · 输入书签关键词",
    },
    app: {
        id: "app",
        label: "应用模式",
        prefix: "r",
        description: "仅搜索本机应用",
        placeholder: "应用模式 · 输入应用名称",
    },
};

export const MODE_LIST: ModeConfig[] = Object.values(MODE_CONFIGS);

const PREFIX_TO_MODE = MODE_LIST.reduce<Record<string, ModeConfig>>((acc, mode) => {
    if (mode.prefix) {
        acc[mode.prefix.toLowerCase()] = mode;
    }
    return acc;
}, {});

export type ModeDetectionResult = {
    mode: ModeConfig;
    cleanedQuery: string;
    isPrefixOnly: boolean;
};

export const detectModeFromInput = (inputValue: string): ModeDetectionResult => {
    const trimmedLeft = inputValue.replace(/^\s+/, "");
    const modeMatch = trimmedLeft.match(/^([a-zA-Z])(?:\s+|:)(.*)$/);

    if (modeMatch) {
        const [, prefixRaw, remainder = ""] = modeMatch;
        const mode = PREFIX_TO_MODE[prefixRaw.toLowerCase()];
        if (mode) {
            const cleaned = remainder.replace(/^\s+/, "");
            return {
                mode,
                cleanedQuery: cleaned,
                isPrefixOnly: cleaned.length === 0,
            };
        }
    }

    return {
        mode: MODE_CONFIGS.all,
        cleanedQuery: inputValue,
        isPrefixOnly: false,
    };
};
