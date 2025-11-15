import {
    useCallback,
    useEffect,
    useMemo,
    useReducer,
    useRef,
} from "react";
import type {
    ChangeEvent,
    CompositionEvent,
    KeyboardEvent as InputKeyboardEvent,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";

import { SearchBar } from "./SearchBar";
import { ResultList } from "./ResultList";
import { PreviewPane } from "./PreviewPane";
import { Toast } from "./Toast";
import { MODE_CONFIGS, detectModeFromInput } from "../constants/modes";
import { HIDE_WINDOW_EVENT, OPEN_SETTINGS_EVENT, SETTINGS_UPDATED_EVENT } from "../constants/events";
import { initialLauncherState, launcherReducer } from "../state/launcherReducer";
import type { AppSettings, SearchResult } from "../types";
import { pickFallbackIcon } from "../utils/fallbackIcon";

export const LauncherWindow = () => {
    const [state, dispatch] = useReducer(launcherReducer, initialLauncherState);
    const searchInputRef = useRef<HTMLInputElement | null>(null);
    const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const latestQueryRef = useRef("");
    const currentWindow = useMemo(() => getCurrentWindow(), []);
    const queryDelayMs = state.settings?.query_delay_ms ?? 120;

    const applyInputValue = useCallback((value: string) => {
        const detection = detectModeFromInput(value);
        dispatch({
            type: "SET_INPUT",
            payload: {
                inputValue: value,
                searchQuery: detection.cleanedQuery,
                activeMode: detection.mode,
                isModePrefixOnly: detection.isPrefixOnly,
            },
        });
    }, []);

    const resetSearchState = useCallback(() => {
        dispatch({ type: "RESET_SEARCH" });
    }, []);

    const showToast = useCallback((message: string) => {
        dispatch({ type: "SET_TOAST", payload: message });
        if (toastTimerRef.current) {
            window.clearTimeout(toastTimerRef.current);
        }
        toastTimerRef.current = window.setTimeout(() => {
            dispatch({ type: "SET_TOAST", payload: null });
            toastTimerRef.current = null;
        }, 3200);
    }, []);

    const loadSettings = useCallback(async () => {
        try {
            const appSettings = await invoke<AppSettings>("get_settings");
            dispatch({ type: "SET_SETTINGS", payload: appSettings });
        } catch (error) {
            console.error("Failed to load settings", error);
            showToast("加载设置失败");
        }
    }, [showToast]);

    const openSettingsWindow = useCallback(async () => {
        const existing = await WebviewWindow.getByLabel("settings");
        if (existing) {
            try {
                await existing.show();
                await existing.setFocus();
                return;
            } catch (error) {
                console.error("Failed to focus settings window", error);
                showToast("设置窗口切换失败");
                return;
            }
        }

        const windowRef = new WebviewWindow("settings", {
            title: "RustLauncher 设置",
            url: "index.html?window=settings",
            width: 420,
            height: 520,
            minWidth: 420,
            minHeight: 460,
            center: true,
            resizable: false,
            decorations: true,
            alwaysOnTop: false,
            transparent: false,
        });

        windowRef.once("tauri://error", (event) => {
            console.error("Settings window error", event.payload);
            showToast("无法打开设置窗口");
        });
    }, [showToast]);

    useEffect(() => {
        return () => {
            if (toastTimerRef.current) {
                window.clearTimeout(toastTimerRef.current);
            }
        };
    }, []);

    useEffect(() => {
        void loadSettings();
    }, [loadSettings]);

    useEffect(() => {
        let unlisten: UnlistenFn | undefined;

        const register = async () => {
            try {
                unlisten = await listen<AppSettings>(SETTINGS_UPDATED_EVENT, (event) => {
                    if (event.payload) {
                        dispatch({ type: "SET_SETTINGS", payload: event.payload });
                    } else {
                        void loadSettings();
                    }
                });
            } catch (error) {
                console.error("Failed to listen settings update", error);
            }
        };

        void register();

        return () => {
            if (unlisten) {
                unlisten();
            }
        };
    }, [loadSettings]);

    useEffect(() => {
        void invoke("trigger_reindex").catch((error: unknown) => {
            console.error("Failed to trigger reindex", error);
            showToast("索引初始化失败");
        });
    }, [showToast]);

    useEffect(() => {
        let unlisten: UnlistenFn | undefined;

        const register = async () => {
            try {
                unlisten = await listen(OPEN_SETTINGS_EVENT, () => {
                    void openSettingsWindow();
                });
            } catch (error) {
                console.error("Failed to listen open settings event", error);
                showToast("设置窗口事件监听失败");
            }
        };

        void register();

        return () => {
            if (unlisten) {
                unlisten();
            }
        };
    }, [openSettingsWindow, showToast]);

    useEffect(() => {
        const handleEsc = (event: KeyboardEvent) => {
            if (event.key === "Escape") {
                void currentWindow.hide();
            }
        };

        window.addEventListener("keydown", handleEsc);

        let unlisten: UnlistenFn | undefined;

        const register = async () => {
            try {
                unlisten = await listen(HIDE_WINDOW_EVENT, () => {
                    resetSearchState();
                    void currentWindow.hide();
                });
            } catch (error) {
                console.error("Failed to listen hide window event", error);
                showToast("窗口事件监听失败");
            }
        };

        void register();

        return () => {
            window.removeEventListener("keydown", handleEsc);
            if (unlisten) {
                unlisten();
            }
        };
    }, [currentWindow, resetSearchState, showToast]);

    useEffect(() => {
        if (state.isComposing || state.isModePrefixOnly) {
            return;
        }

        latestQueryRef.current = state.searchQuery;
        const trimmed = state.searchQuery.trim();

        if (!trimmed) {
            dispatch({ type: "SET_RESULTS", payload: [] });
            dispatch({ type: "SET_SELECTED_INDEX", payload: 0 });
            return;
        }

        const payload: { query: string; mode?: string } = { query: trimmed };
        if (state.activeMode.id !== MODE_CONFIGS.all.id) {
            payload.mode = state.activeMode.id;
        }

        const timeoutId = window.setTimeout(async () => {
            try {
                const newResults = await invoke<SearchResult[]>("submit_query", payload);
                if (latestQueryRef.current === state.searchQuery) {
                    dispatch({ type: "SET_RESULTS", payload: newResults });
                    dispatch({ type: "SET_SELECTED_INDEX", payload: 0 });
                }
            } catch (error) {
                console.error("Failed to query", error);
                showToast("搜索失败，请稍后重试");
            }
        }, queryDelayMs);

        return () => {
            window.clearTimeout(timeoutId);
        };
    }, [state.searchQuery, state.activeMode, state.isComposing, state.isModePrefixOnly, showToast, queryDelayMs]);

    const executeSelected = useCallback(
        async (selected?: SearchResult) => {
            if (!selected) {
                return;
            }

            try {
                await invoke("execute_action", {
                    id: selected.id,
                });
                resetSearchState();
            } catch (error) {
                console.error("Failed to execute action", error);
                showToast("执行失败，请检查目标是否存在");
            }
        },
        [resetSearchState, showToast],
    );

    const stepSelection = useCallback(
        (direction: 1 | -1) => {
            const resultsCount = state.results.length;
            if (resultsCount === 0) {
                dispatch({ type: "SET_SELECTED_INDEX", payload: 0 });
                return;
            }
            dispatch({
                type: "SET_SELECTED_INDEX",
                payload: (state.selectedIndex + direction + resultsCount) % resultsCount,
            });
        },
        [state.results.length, state.selectedIndex],
    );

    const handleKeyDown = useCallback(
        (event: InputKeyboardEvent<HTMLInputElement>) => {
            if (event.key === "ArrowDown") {
                event.preventDefault();
                stepSelection(1);
                return;
            }

            if (event.key === "ArrowUp") {
                event.preventDefault();
                stepSelection(-1);
                return;
            }

            if (event.key === "Enter") {
                event.preventDefault();
                void executeSelected(state.results[state.selectedIndex]);
            }
        },
        [executeSelected, state.results, state.selectedIndex, stepSelection],
    );

    const resolveResultTag = useCallback((item: SearchResult) => {
        switch (item.action_id) {
            case "app":
            case "uwp":
                return "应用";
            case "bookmark":
                return "书签";
            case "url":
                return "网址";
            case "search":
                return "搜索";
            default:
                return "其他";
        }
    }, []);

    const handleSettingsButtonClick = useCallback(() => {
        void openSettingsWindow();
    }, [openSettingsWindow]);

    const handleResultSelect = useCallback((index: number) => {
        dispatch({ type: "SET_SELECTED_INDEX", payload: index });
    }, []);

    const handleResultActivate = useCallback(
        (item: SearchResult) => {
            void executeSelected(item);
        },
        [executeSelected],
    );

    const resultsCount = state.results.length;
    const trimmedInput = state.inputValue.trim();
    const hasQuery = trimmedInput.length > 0;
    const hasMatches = resultsCount > 0;
    const activeResult = hasMatches ? state.results[state.selectedIndex] : null;
    const fallbackVisual = activeResult ? pickFallbackIcon(activeResult) : null;
    const activeResultTag = activeResult ? resolveResultTag(activeResult) : null;
    const disableNav = resultsCount <= 1;
    const showPreviewPane = state.settings?.enable_preview_panel ?? true;
    const contentAreaClassName = showPreviewPane ? "content-area" : "content-area content-area--single";
    const isIdle = !hasQuery && !state.isModePrefixOnly;
    const windowClassName = isIdle ? "flow-window flow-window--compact" : "flow-window";

    return (
        <div className={windowClassName} data-tauri-drag-region>
            <section className={isIdle ? "search-area search-area--solo" : "search-area"}>
                <SearchBar
                    value={state.inputValue}
                    placeholder={state.activeMode.placeholder}
                    inputRef={searchInputRef}
                    onChange={(event: ChangeEvent<HTMLInputElement>) =>
                        applyInputValue(event.currentTarget.value)
                    }
                    onCompositionStart={(_event: CompositionEvent<HTMLInputElement>) => {
                        dispatch({ type: "SET_COMPOSING", payload: true });
                    }}
                    onCompositionEnd={(event: CompositionEvent<HTMLInputElement>) => {
                        dispatch({ type: "SET_COMPOSING", payload: false });
                        applyInputValue(event.currentTarget.value);
                    }}
                    onKeyDown={handleKeyDown}
                />
                {state.isModePrefixOnly ? (
                    <div className="mode-prefix-hint">
                        已切换至 {state.activeMode.label}，请输入关键词开始搜索
                    </div>
                ) : null}
            </section>
            {isIdle ? null : (
                <section className={contentAreaClassName}>
                    <div className="results-panel">
                        {hasMatches ? (
                            <ResultList
                                results={state.results}
                                selectedIndex={state.selectedIndex}
                                onSelect={handleResultSelect}
                                onActivate={handleResultActivate}
                                resolveResultTag={resolveResultTag}
                            />
                        ) : (
                            <div className="empty-hint">
                                {hasQuery
                                    ? state.activeMode.id === "all"
                                        ? "没有匹配的结果"
                                        : `当前 ${state.activeMode.label} 中没有找到匹配项`
                                    : "输入任意关键词开始搜索"}
                            </div>
                        )}
                        <div className="status-row">
                            <span>{hasMatches ? "Enter · 打开 / ↑↓ · 浏览" : "Alt+Space 唤起 · Esc 隐藏"}</span>
                            <span>
                                {resultsCount === 0
                                    ? "00 / 00"
                                    : `${String(state.selectedIndex + 1).padStart(2, "0")} / ${String(resultsCount).padStart(2, "0")}`}
                            </span>
                        </div>
                    </div>
                    {showPreviewPane ? (
                        <PreviewPane
                            result={activeResult}
                            fallbackVisual={fallbackVisual}
                            tagLabel={activeResultTag}
                            onPrev={() => stepSelection(-1)}
                            onNext={() => stepSelection(1)}
                            onExecute={() => {
                                if (activeResult) {
                                    void executeSelected(activeResult);
                                }
                            }}
                            disableNavigation={disableNav}
                        />
                    ) : (
                        <div className="preview-panel preview-panel--ghost">
                            <div className="preview-placeholder">
                                <div className="preview-title">预览面板已隐藏</div>
                                <div className="preview-subtitle">前往 设置 → 外观 可重新启用</div>
                                <button
                                    type="button"
                                    className="ghost-button"
                                    onClick={handleSettingsButtonClick}
                                >
                                    打开设置
                                </button>
                            </div>
                        </div>
                    )}
                </section>
            )}
            {state.toastMessage ? <Toast message={state.toastMessage} /> : null}
        </div>
    );
};
