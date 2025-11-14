import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
  ChangeEvent,
  CompositionEvent,
  KeyboardEvent as InputKeyboardEvent,
  MouseEvent as ListMouseEvent,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import "./App.css";

type SearchResult = {
  id: string;
  title: string;
  subtitle: string;
  icon: string;
  score: number;
  action_id: string;
  action_payload: string;
};

type AppSettings = {
  global_hotkey: string;
};

const HIDE_WINDOW_EVENT = "hide_window";
const OPEN_SETTINGS_EVENT = "open_settings";
function App() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [isComposing, setIsComposing] = useState(false);
  const [toastMessage, setToastMessage] = useState<string | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [hotkeyInput, setHotkeyInput] = useState("");
  const [isSavingHotkey, setIsSavingHotkey] = useState(false);
  const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const settingsInputRef = useRef<HTMLInputElement | null>(null);
  const latestQueryRef = useRef("");
  const currentWindow = useMemo(() => getCurrentWindow(), []);

  const showToast = useCallback((message: string) => {
    setToastMessage(message);
    if (toastTimerRef.current !== null) {
      window.clearTimeout(toastTimerRef.current);
    }
    toastTimerRef.current = window.setTimeout(() => {
      setToastMessage(null);
      toastTimerRef.current = null;
    }, 3200);
  }, []);

  const loadSettings = useCallback(async () => {
    try {
      const appSettings = await invoke<AppSettings>("get_settings");
      setSettings(appSettings);
      setHotkeyInput(appSettings.global_hotkey);
    } catch (error) {
      console.error("Failed to load settings", error);
      showToast("加载设置失败");
    }
  }, [showToast]);

  useEffect(() => {
    return () => {
      if (toastTimerRef.current !== null) {
        window.clearTimeout(toastTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    void loadSettings();
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
          setIsSettingsOpen(true);
          void loadSettings();
        });
      } catch (error) {
        console.error("Failed to listen open settings event", error);
        showToast("设置事件监听失败");
      }
    };

    void register();

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, [loadSettings, showToast]);

  useEffect(() => {
    const handleEsc = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (isSettingsOpen) {
          setIsSettingsOpen(false);
          return;
        }
        void currentWindow.hide();
      }
    };

    window.addEventListener("keydown", handleEsc);

    let unlisten: UnlistenFn | undefined;

    const register = async () => {
      try {
        unlisten = await listen(HIDE_WINDOW_EVENT, () => {
          setQuery("");
          setResults([]);
          setSelectedIndex(0);
          setIsSettingsOpen(false);
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
  }, [currentWindow, isSettingsOpen, showToast]);

  useEffect(() => {
    if (isSettingsOpen && settingsInputRef.current) {
      settingsInputRef.current.focus();
      settingsInputRef.current.select();
    }
  }, [isSettingsOpen]);

  useEffect(() => {
    if (isComposing) {
      return;
    }

    latestQueryRef.current = query;
    const trimmed = query.trim();

    if (!trimmed) {
      setResults([]);
      setSelectedIndex(0);
      return;
    }

    const timeoutId = window.setTimeout(async () => {
      try {
        const newResults = await invoke<SearchResult[]>("submit_query", {
          query,
        });
        if (latestQueryRef.current === query) {
          setResults(newResults);
          setSelectedIndex(0);
        }
      } catch (error) {
        console.error("Failed to query", error);
        showToast("搜索失败，请稍后重试");
      }
    }, 120);

    return () => {
      window.clearTimeout(timeoutId);
    };
  }, [query, isComposing, showToast]);

  const handleKeyDown = useCallback(
    async (event: InputKeyboardEvent<HTMLInputElement>) => {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setSelectedIndex((current: number) =>
          Math.min(current + 1, Math.max(results.length - 1, 0)),
        );
        return;
      }

      if (event.key === "ArrowUp") {
        event.preventDefault();
        setSelectedIndex((current: number) => Math.max(current - 1, 0));
        return;
      }

      if (event.key === "Enter") {
        event.preventDefault();
        const selected = results[selectedIndex];
        if (selected) {
          try {
            await invoke("execute_action", {
              id: selected.action_id,
              payload: selected.action_payload,
            });
            setQuery("");
            setResults([]);
            setSelectedIndex(0);
          } catch (error) {
            console.error("Failed to execute action", error);
            showToast("执行失败，请检查目标是否存在");
          }
        }
      }
    },
    [results, selectedIndex, showToast],
  );

  const handleMouseClick = useCallback(
    async (index: number) => {
      const selected = results[index];
      if (!selected) {
        return;
      }

      try {
        await invoke("execute_action", {
          id: selected.action_id,
          payload: selected.action_payload,
        });
        setQuery("");
        setResults([]);
        setSelectedIndex(0);
      } catch (error) {
        console.error("Failed to execute action", error);
        showToast("执行失败，请检查目标是否存在");
      }
    },
    [results, showToast],
  );

  const handleSettingsButtonClick = useCallback(() => {
    setIsSettingsOpen((current) => {
      if (current) {
        return false;
      }
      void loadSettings();
      return true;
    });
  }, [loadSettings]);

  const handleSettingsClose = useCallback(() => {
    setIsSettingsOpen(false);
  }, []);

  const handleHotkeySave = useCallback(async () => {
    const trimmed = hotkeyInput.trim();
    if (!trimmed) {
      showToast("快捷键不能为空");
      return;
    }

    try {
      setIsSavingHotkey(true);
      const updated = await invoke<AppSettings>("update_hotkey", {
        hotkey: trimmed,
      });
      setSettings(updated);
      setHotkeyInput(updated.global_hotkey);
      showToast("全局热键已更新");
    } catch (error) {
      console.error("Failed to update hotkey", error);
      showToast("更新热键失败");
    } finally {
      setIsSavingHotkey(false);
    }
  }, [hotkeyInput, showToast]);

  const handleHotkeyKeyDown = useCallback(
    (event: InputKeyboardEvent<HTMLInputElement>) => {
      if (event.key === "Enter") {
        event.preventDefault();
        void handleHotkeySave();
      }
    },
    [handleHotkeySave],
  );

  return (
    <div className="container" data-tauri-drag-region>
      <button
        type="button"
        className="settings-button"
        onClick={handleSettingsButtonClick}
        aria-label={isSettingsOpen ? "关闭设置" : "打开设置"}
      >
        <span aria-hidden="true">⚙</span>
      </button>
      <input
        type="text"
        className="search-bar"
        value={query}
        onChange={(event: ChangeEvent<HTMLInputElement>) =>
          setQuery(event.currentTarget.value)
        }
        onCompositionStart={(_event: CompositionEvent<HTMLInputElement>) =>
          setIsComposing(true)
        }
        onCompositionEnd={(event: CompositionEvent<HTMLInputElement>) => {
          setIsComposing(false);
          setQuery(event.currentTarget.value);
        }}
        onKeyDown={handleKeyDown}
        placeholder="搜索应用和网页..."
        autoFocus
      />
      <div className="search-hint">支持拼音全拼及首字母缩写匹配</div>
      {results.length > 0 ? (
        <ul className="results-list">
          {results.map((item: SearchResult, index: number) => (
            <li
              key={item.id}
              className={
                index === selectedIndex ? "result-item selected" : "result-item"
              }
              onMouseEnter={() => setSelectedIndex(index)}
              onMouseDown={(event: ListMouseEvent<HTMLLIElement>) =>
                event.preventDefault()
              }
              onClick={() => void handleMouseClick(index)}
            >
              {item.icon ? (
                <img
                  src={`data:image/png;base64,${item.icon}`}
                  className="result-icon"
                  alt="result icon"
                />
              ) : (
                <div className="result-icon placeholder" />
              )}
              <div className="result-text">
                <div className="result-title-row">
                  <div className="result-title">{item.title}</div>
                  {item.action_id === "bookmark" ? (
                    <span className="result-tag">书签</span>
                  ) : null}
                </div>
                <div className="result-subtitle">{item.subtitle}</div>
              </div>
            </li>
          ))}
        </ul>
      ) : (
        <div className="empty-state">开始输入以搜索应用或网页</div>
      )}
      {isSettingsOpen ? (
        <button
          type="button"
          className="settings-overlay"
          aria-label="关闭设置"
          onClick={handleSettingsClose}
        />
      ) : null}
      <div
        className={isSettingsOpen ? "settings-panel open" : "settings-panel"}
        aria-hidden={!isSettingsOpen}
      >
        <div className="settings-heading">
          <div>
            <div className="settings-title">设置</div>
            <div className="settings-subtitle">
              当前快捷键：{settings?.global_hotkey ?? "加载中..."}
            </div>
          </div>
        </div>
        <div className="settings-field">
          <label htmlFor="hotkey-input">全局快捷键</label>
          <input
            id="hotkey-input"
            type="text"
            ref={settingsInputRef}
            value={hotkeyInput}
            onChange={(event: ChangeEvent<HTMLInputElement>) =>
              setHotkeyInput(event.currentTarget.value)
            }
            onKeyDown={handleHotkeyKeyDown}
            placeholder="例如 Alt+Space"
            className="settings-input"
          />
          <span className="settings-hint">
            用 + 连接组合键，例如 Ctrl+Shift+P
          </span>
        </div>
        <div className="settings-actions">
          <button
            type="button"
            className="ghost-button"
            onClick={handleSettingsClose}
          >
            取消
          </button>
          <button
            type="button"
            className="primary-button"
            onClick={() => void handleHotkeySave()}
            disabled={isSavingHotkey}
          >
            {isSavingHotkey ? "保存中..." : "保存"}
          </button>
        </div>
      </div>
      {toastMessage ? <div className="toast">{toastMessage}</div> : null}
    </div>
  );
}

export default App;
