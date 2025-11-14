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

const HIDE_WINDOW_EVENT = "hide_window";

function App() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [isComposing, setIsComposing] = useState(false);
  const [toastMessage, setToastMessage] = useState<string | null>(null);
  const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
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

  useEffect(() => {
    return () => {
      if (toastTimerRef.current !== null) {
        window.clearTimeout(toastTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    void invoke("trigger_reindex").catch((error: unknown) => {
      console.error("Failed to trigger reindex", error);
      showToast("索引初始化失败");
    });
  }, [showToast]);

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
          setQuery("");
          setResults([]);
          setSelectedIndex(0);
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
  }, [currentWindow, showToast]);

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

  return (
    <div className="container" data-tauri-drag-region>
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
                <div className="result-title">{item.title}</div>
                <div className="result-subtitle">{item.subtitle}</div>
              </div>
            </li>
          ))}
        </ul>
      ) : (
        <div className="empty-state">开始输入以搜索应用或网页</div>
      )}
      {toastMessage ? <div className="toast">{toastMessage}</div> : null}
    </div>
  );
}

export default App;
