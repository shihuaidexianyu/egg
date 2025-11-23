import { useCallback, useEffect, useMemo, useReducer, useRef } from "react";
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
import { LogicalSize, LogicalPosition } from "@tauri-apps/api/dpi";

import { SearchBar } from "./SearchBar";
import { ResultList } from "./ResultList";
import { Toast } from "./Toast";
import {
  buildModeConfigsFromSettings,
  buildPrefixToMode,
  detectModeFromInput,
} from "../constants/modes";
import {
  FOCUS_INPUT_EVENT,
  HIDE_WINDOW_EVENT,
  OPEN_SETTINGS_EVENT,
  SETTINGS_UPDATED_EVENT,
  WINDOW_OPACITY_PREVIEW_EVENT,
} from "../constants/events";
import {
  initialLauncherState,
  launcherReducer,
} from "../state/launcherReducer";
import type { AppSettings, SearchResult } from "../types";
import { applyWindowOpacityVariable } from "../utils/theme";

const SETTINGS_WINDOW_LABEL = "settings";

type WindowOpacityPreviewEventPayload =
  | number
  | {
    value?: number;
    temporary?: boolean;
  };

let trackedSettingsWindow: WebviewWindow | null = null;

const resolveTrackedSettingsWindow =
  async (): Promise<WebviewWindow | null> => {
    if (trackedSettingsWindow) {
      return trackedSettingsWindow;
    }

    try {
      const maybeWindow = await Promise.resolve(
        WebviewWindow.getByLabel(SETTINGS_WINDOW_LABEL),
      );
      if (maybeWindow) {
        trackedSettingsWindow = maybeWindow;
        return maybeWindow;
      }
    } catch (error) {
      console.error("Failed to lookup existing settings window", error);
    }

    return null;
  };

export const LauncherWindow = () => {
  const [state, dispatch] = useReducer(launcherReducer, initialLauncherState);
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const latestQueryRef = useRef("");
  const currentWindow = useMemo(() => getCurrentWindow(), []);
  const queryDelayMs = state.settings?.query_delay_ms ?? 120;
  const modeConfigs = useMemo(
    () => buildModeConfigsFromSettings(state.settings),
    [state.settings],
  );
  const prefixToMode = useMemo(
    () => buildPrefixToMode(modeConfigs),
    [modeConfigs],
  );

  const applyInputValue = useCallback(
    (value: string) => {
      const detection = detectModeFromInput(value, prefixToMode);
      dispatch({
        type: "SET_INPUT",
        payload: {
          inputValue: value,
          searchQuery: detection.cleanedQuery,
          activeMode: detection.mode,
          isModePrefixOnly: detection.isPrefixOnly,
        },
      });
    },
    [prefixToMode],
  );

  const focusSearchInput = useCallback(() => {
    const inputElement = searchInputRef.current;
    if (!inputElement) {
      return;
    }

    inputElement.focus();
    const caretPosition = inputElement.value.length;
    try {
      inputElement.setSelectionRange(caretPosition, caretPosition);
    } catch (_error) {
      // 某些输入法可能不支持 setSelectionRange，这里忽略异常即可
    }
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
    const existing = await resolveTrackedSettingsWindow();
    if (existing) {
      try {
        await existing.show();
        await existing.setFocus();
        return;
      } catch (error) {
        console.error("Failed to focus settings window", error);
        showToast("设置窗口切换失败");
        trackedSettingsWindow = null;
      }
    }

    const windowRef = new WebviewWindow(SETTINGS_WINDOW_LABEL, {
      title: "egg 设置",
      url: "index.html?window=settings",
      width: 960,
      height: 640,
      minWidth: 760,
      minHeight: 520,
      resizable: true,
      center: true,
      decorations: true,
      alwaysOnTop: false,
      transparent: false,
    });
    trackedSettingsWindow = windowRef;

    windowRef.once("tauri://destroyed", () => {
      if (trackedSettingsWindow?.label === SETTINGS_WINDOW_LABEL) {
        trackedSettingsWindow = null;
      }
    });

    windowRef.once("tauri://error", (event) => {
      console.error("Settings window error", event.payload);
      showToast("无法打开设置窗口");
      if (trackedSettingsWindow?.label === SETTINGS_WINDOW_LABEL) {
        trackedSettingsWindow = null;
      }
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
    const opacity = state.settings?.window_opacity ?? 0.95;
    applyWindowOpacityVariable(opacity);
  }, [state.settings?.window_opacity]);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;

    const register = async () => {
      try {
        unlisten = await listen<WindowOpacityPreviewEventPayload>(
          WINDOW_OPACITY_PREVIEW_EVENT,
          (event) => {
            const payload = event.payload;
            const value =
              typeof payload === "number"
                ? payload
                : typeof payload?.value === "number"
                  ? payload.value
                  : null;
            if (typeof value !== "number" || Number.isNaN(value)) {
              return;
            }
            applyWindowOpacityVariable(value);
          },
        );
      } catch (error) {
        console.error("Failed to listen window opacity preview event", error);
      }
    };

    void register();

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

  useEffect(() => {
    focusSearchInput();
  }, [focusSearchInput]);

  useEffect(() => {
    const handleContextMenu = (event: MouseEvent) => {
      if (!(state.settings?.debug_mode ?? false)) {
        event.preventDefault();
      }
    };

    window.addEventListener("contextmenu", handleContextMenu);
    return () => {
      window.removeEventListener("contextmenu", handleContextMenu);
    };
  }, [state.settings?.debug_mode]);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;

    const register = async () => {
      try {
        unlisten = await listen(FOCUS_INPUT_EVENT, () => {
          focusSearchInput();
        });
      } catch (error) {
        console.error("Failed to listen focus input event", error);
      }
    };

    void register();

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, [focusSearchInput]);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;

    const register = async () => {
      try {
        unlisten = await listen<AppSettings>(
          SETTINGS_UPDATED_EVENT,
          (event) => {
            if (event.payload) {
              dispatch({ type: "SET_SETTINGS", payload: event.payload });
            } else {
              void loadSettings();
            }
          },
        );
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
        // 按下 Esc 时也走统一的“重置 + 隐藏”事件通路
        void invoke("emit", { event: HIDE_WINDOW_EVENT, payload: null });
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
    if (state.activeMode.id !== modeConfigs.all.id) {
      payload.mode = state.activeMode.id;
    }

    const timeoutId = window.setTimeout(async () => {
      try {
        const newResults = await invoke<SearchResult[]>(
          "submit_query",
          payload,
        );
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
  }, [
    state.searchQuery,
    state.activeMode,
    state.isComposing,
    state.isModePrefixOnly,
    showToast,
    queryDelayMs,
  ]);

  const executeSelected = useCallback(
    async (selected?: SearchResult, runAsAdmin: boolean = false) => {
      if (!selected) {
        return;
      }

      try {
        await invoke("execute_action", {
          id: selected.id,
          runAsAdmin,
        });
        // 执行完成后，通过统一的隐藏事件让窗口隐藏并重置搜索
        const hideEvent = new CustomEvent(HIDE_WINDOW_EVENT);
        window.dispatchEvent(hideEvent);
      } catch (error) {
        console.error("Failed to execute action", error);
        showToast("执行失败，请检查目标是否存在");
      }
    },
    [showToast],
  );

  const stepSelection = useCallback(
    (direction: 1 | -1) => {
      const resultsCount = state.results.length;
      if (resultsCount === 0) {
        dispatch({ type: "SET_SELECTED_INDEX", payload: 0 });
        return;
      }
      const nextIndex = state.selectedIndex + direction;
      if (nextIndex < 0 || nextIndex >= resultsCount) {
        return;
      }
      dispatch({
        type: "SET_SELECTED_INDEX",
        payload: nextIndex,
      });
    },
    [state.results.length, state.selectedIndex],
  );

  const handleKeyDown = useCallback(
    (event: InputKeyboardEvent<HTMLInputElement>) => {
      if ((event.ctrlKey || event.metaKey) && event.key === ",") {
        event.preventDefault();
        void openSettingsWindow();
        return;
      }
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
        const runAsAdmin = event.ctrlKey || event.metaKey;
        void executeSelected(state.results[state.selectedIndex], runAsAdmin);
      }
    },
    [
      executeSelected,
      openSettingsWindow,
      state.results,
      state.selectedIndex,
      stepSelection,
    ],
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
  const isIdle = !hasQuery && !state.isModePrefixOnly;
  const windowClassName = isIdle
    ? "flow-window flow-window--compact"
    : "flow-window";

  useEffect(() => {
    const BASE_HEIGHT = 62;
    const ITEM_HEIGHT = 52;
    const LIST_PADDING = 0;
    const MAX_HEIGHT = 460;

    let targetHeight = BASE_HEIGHT;

    // Only expand if there are actual results
    if (!isIdle && hasMatches && resultsCount > 0) {
      const contentHeight = resultsCount * ITEM_HEIGHT + LIST_PADDING;
      targetHeight = Math.min(MAX_HEIGHT, BASE_HEIGHT + contentHeight);
    }

    const resizeWindow = async () => {
      try {
        const WINDOW_WIDTH = 800;
        const newSize = new LogicalSize(WINDOW_WIDTH, targetHeight);
        await currentWindow.setSize(newSize);

        const monitor = await currentWindow.currentMonitor();
        if (monitor) {
          const screenWidth = monitor.size.width;
          const screenHeight = monitor.size.height;
          const scaleFactor = monitor.scaleFactor;

          const logicalScreenWidth = screenWidth / scaleFactor;
          const logicalScreenHeight = screenHeight / scaleFactor;

          const x = (logicalScreenWidth - WINDOW_WIDTH) / 2;
          const y = logicalScreenHeight * 0.25;

          await currentWindow.setPosition(new LogicalPosition(x, y));
        }

        console.log(`Resized to ${WINDOW_WIDTH}x${targetHeight}, ${resultsCount} results`);
      } catch (error) {
        console.error("Failed to resize window:", error);
      }
    };

    void resizeWindow();
  }, [isIdle, resultsCount, currentWindow]);

  return (
    <div className={windowClassName} data-tauri-drag-region>
      <section
        className={isIdle ? "search-area search-area--solo" : "search-area"}
      >
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
          rightContent={
            hasQuery && !state.isComposing ? (
              <div className="search-status-area">
                <span className="search-mode-label">{state.activeMode.label}</span>
                <div className={`search-status-indicator ${hasMatches ? 'success' : 'error'}`}></div>
              </div>
            ) : null
          }
        />
      </section>
      {!isIdle && hasMatches ? (
        <section className="content-area content-area--single">
          <div className="results-panel">
            <div className="results-separator"></div>
            <ResultList
              results={state.results}
              selectedIndex={state.selectedIndex}
              onSelect={handleResultSelect}
              onActivate={handleResultActivate}
              resolveResultTag={resolveResultTag}
            />
          </div>
        </section>
      ) : null}
      {state.toastMessage ? <Toast message={state.toastMessage} /> : null}
    </div>
  );
};
