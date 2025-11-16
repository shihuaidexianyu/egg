import type { AppSettings, ModeConfig, ModeId } from "../types";

// 默认模式配置（在未加载设置或缺省前缀时使用）
export const DEFAULT_MODE_CONFIGS: Record<ModeId, ModeConfig> = {
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
  search: {
    id: "search",
    label: "搜索模式",
    prefix: "s",
    description: "仅使用网络搜索",
    placeholder: "搜索模式 · 输入关键词，在浏览器中搜索",
  },
};

// 根据设置构造当前模式配置，允许自定义前缀
export const buildModeConfigsFromSettings = (
  settings: AppSettings | null,
): Record<ModeId, ModeConfig> => {
  if (!settings) {
    return DEFAULT_MODE_CONFIGS;
  }

  return {
    all: DEFAULT_MODE_CONFIGS.all,
    bookmark: {
      ...DEFAULT_MODE_CONFIGS.bookmark,
      prefix: settings.prefix_bookmark || DEFAULT_MODE_CONFIGS.bookmark.prefix,
    },
    app: {
      ...DEFAULT_MODE_CONFIGS.app,
      prefix: settings.prefix_app || DEFAULT_MODE_CONFIGS.app.prefix,
    },
    search: {
      ...DEFAULT_MODE_CONFIGS.search,
      prefix: settings.prefix_search || DEFAULT_MODE_CONFIGS.search.prefix,
    },
  };
};

export const MODE_LIST: ModeConfig[] = Object.values(DEFAULT_MODE_CONFIGS);

export const buildPrefixToMode = (
  configs: Record<ModeId, ModeConfig>,
): Record<string, ModeConfig> => {
  return Object.values(configs).reduce<Record<string, ModeConfig>>(
    (acc, mode) => {
      if (mode.prefix) {
        acc[mode.prefix.toLowerCase()] = mode;
      }
      return acc;
    },
    {},
  );
};

export type ModeDetectionResult = {
  mode: ModeConfig;
  cleanedQuery: string;
  isPrefixOnly: boolean;
};

export const detectModeFromInput = (
  inputValue: string,
  prefixToMode: Record<string, ModeConfig>,
): ModeDetectionResult => {
  const trimmedLeft = inputValue.replace(/^\s+/, "");
  if (!trimmedLeft) {
    return {
      mode: DEFAULT_MODE_CONFIGS.all,
      cleanedQuery: "",
      isPrefixOnly: false,
    };
  }

  const lowerInput = trimmedLeft.toLowerCase();
  const sortedEntries = Object.entries(prefixToMode).sort(
    (a, b) => b[0].length - a[0].length,
  );

  for (const [rawPrefix, mode] of sortedEntries) {
    const normalizedPrefix = rawPrefix.toLowerCase();
    if (!normalizedPrefix) {
      continue;
    }

    const suffixChar = normalizedPrefix.slice(-1);
    const hasInlineDelimiter = suffixChar === " " || suffixChar === ":";

    if (hasInlineDelimiter) {
      if (lowerInput.startsWith(normalizedPrefix)) {
        const remainder = trimmedLeft.slice(normalizedPrefix.length);
        const cleaned = remainder.replace(/^\s+/, "");
        return {
          mode,
          cleanedQuery: cleaned,
          isPrefixOnly: cleaned.length === 0,
        };
      }
      continue;
    }

    if (!lowerInput.startsWith(normalizedPrefix)) {
      continue;
    }

    const remainder = trimmedLeft.slice(normalizedPrefix.length);
    if (remainder && !/^[:\s]/.test(remainder)) {
      continue;
    }

    const cleaned = remainder.replace(/^[:\s]+/, "");
    return {
      mode,
      cleanedQuery: cleaned,
      isPrefixOnly: cleaned.length === 0,
    };
  }

  return {
    mode: DEFAULT_MODE_CONFIGS.all,
    cleanedQuery: inputValue,
    isPrefixOnly: false,
  };
};
