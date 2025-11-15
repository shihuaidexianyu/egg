export type SearchResult = {
    id: string;
    title: string;
    subtitle: string;
    icon: string;
    score: number;
    action_id: string;
};

export type AppSettings = {
    global_hotkey: string;
    query_delay_ms: number;
    max_results: number;
    enable_preview_panel: boolean;
    enable_app_results: boolean;
    enable_bookmark_results: boolean;
};

export type ModeId = "all" | "bookmark" | "app";

export type ModeConfig = {
    id: ModeId;
    label: string;
    prefix?: string;
    description: string;
    placeholder: string;
};

export type FallbackVisual = {
    glyph: string;
    background: string;
    color: string;
};

export type LauncherState = {
    inputValue: string;
    searchQuery: string;
    results: SearchResult[];
    selectedIndex: number;
    toastMessage: string | null;
    settings: AppSettings | null;
    activeMode: ModeConfig;
    isModePrefixOnly: boolean;
    isComposing: boolean;
};

export type LauncherAction =
    | {
        type: "SET_INPUT";
        payload: {
            inputValue: string;
            searchQuery: string;
            activeMode: ModeConfig;
            isModePrefixOnly: boolean;
        };
    }
    | { type: "SET_RESULTS"; payload: SearchResult[] }
    | { type: "SET_SELECTED_INDEX"; payload: number }
    | { type: "SET_TOAST"; payload: string | null }
    | { type: "SET_SETTINGS"; payload: AppSettings }
    | { type: "SET_COMPOSING"; payload: boolean }
    | { type: "RESET_SEARCH" };
