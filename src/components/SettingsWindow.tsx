import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ChangeEvent, KeyboardEvent as InputKeyboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import type { AppSettings } from "../types";
import { Toast } from "./Toast";
import { applyWindowOpacityVariable } from "../utils/theme";

const MIN_QUERY_DELAY = 50;
const MAX_QUERY_DELAY = 2000;
const MIN_RESULT_LIMIT = 10;
const MAX_RESULT_LIMIT = 60;
const MIN_WINDOW_OPACITY_PERCENT = 60;
const MAX_WINDOW_OPACITY_PERCENT = 100;
const MIN_WINDOW_OPACITY = MIN_WINDOW_OPACITY_PERCENT / 100;
const MAX_WINDOW_OPACITY = MAX_WINDOW_OPACITY_PERCENT / 100;
type HotkeyCapturePayload = {
  shortcut: string;
};

type SettingsSectionId = "general" | "search" | "about";

const SECTION_DEFS: Array<{
  id: SettingsSectionId;
  label: string;
  description: string;
  icon: string;
}> = [
    { id: "general", label: "常规", description: "呼出快捷键 & 防抖", icon: "⌘" },
    { id: "search", label: "搜索", description: "结果来源 / 数量", icon: "🔍" },
    { id: "about", label: "关于", description: "版本与状态", icon: "ℹ️" },
  ];

type BooleanSettingKey =
  | "enable_app_results"
  | "enable_bookmark_results"
  | "launch_on_startup"
  | "force_english_input"
  | "debug_mode";

const TRACKED_SETTING_KEYS: Array<keyof AppSettings> = [
  "global_hotkey",
  "query_delay_ms",
  "max_results",
  "enable_app_results",
  "enable_bookmark_results",
  "prefix_app",
  "prefix_bookmark",
  "prefix_search",
  "launch_on_startup",
  "force_english_input",
  "debug_mode",
  "window_opacity",
];

export const SettingsWindow = () => {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [draft, setDraft] = useState<AppSettings | null>(null);
  const [activeSection, setActiveSection] =
    useState<SettingsSectionId>("general");
  const [isSaving, setIsSaving] = useState(false);
  const [toastMessage, setToastMessage] = useState<string | null>(null);
  const [appVersion, setAppVersion] = useState("--");
  const [isCapturingHotkey, setIsCapturingHotkey] = useState(false);
  const [hotkeyInputValue, setHotkeyInputValue] = useState("");
  const [isHotkeyEditing, setIsHotkeyEditing] = useState(false);
  const hotkeyInputRef = useRef<HTMLInputElement | null>(null);
  const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const debugModeEffective = draft?.debug_mode ?? settings?.debug_mode ?? false;

  const showToast = useCallback((message: string) => {
    setToastMessage(message);
    if (toastTimerRef.current) {
      window.clearTimeout(toastTimerRef.current);
    }
    toastTimerRef.current = window.setTimeout(() => {
      setToastMessage(null);
      toastTimerRef.current = null;
    }, 2800);
  }, []);

  const loadSettings = useCallback(async () => {
    try {
      const appSettings = await invoke<AppSettings>("get_settings");
      setSettings(appSettings);
      setDraft({ ...appSettings });
      applyWindowOpacityVariable(appSettings.window_opacity);
    } catch (error) {
      console.error("Failed to load settings", error);
      showToast("加载设置失败");
    }
  }, [showToast]);

  useEffect(() => {
    void loadSettings();
    void getVersion().then(setAppVersion).catch(console.error);
    return () => {
      if (toastTimerRef.current) {
        window.clearTimeout(toastTimerRef.current);
      }
    };
  }, [loadSettings]);

  useEffect(() => {
    const handleContextMenu = (event: MouseEvent) => {
      if (!debugModeEffective) {
        event.preventDefault();
      }
    };

    window.addEventListener("contextmenu", handleContextMenu);
    return () => {
      window.removeEventListener("contextmenu", handleContextMenu);
    };
  }, [debugModeEffective]);

  useEffect(() => {
    if (!isCapturingHotkey || !hotkeyInputRef.current) {
      return;
    }
    const input = hotkeyInputRef.current;
    input.focus();
    input.select();
  }, [isCapturingHotkey]);

  useEffect(() => {
    if (!isCapturingHotkey) {
      return;
    }
    const handleWindowBlur = () => {
      setIsCapturingHotkey(false);
      void invoke("end_hotkey_capture");
    };
    window.addEventListener("blur", handleWindowBlur);
    return () => {
      window.removeEventListener("blur", handleWindowBlur);
    };
  }, [isCapturingHotkey]);

  const initialHotkeyFocusDone = useRef(false);

  useEffect(() => {
    if (initialHotkeyFocusDone.current) {
      return;
    }
    if (hotkeyInputRef.current && draft) {
      hotkeyInputRef.current.focus();
      hotkeyInputRef.current.select();
      initialHotkeyFocusDone.current = true;
    }
  }, [draft]);

  useEffect(() => {
    if (draft) {
      applyWindowOpacityVariable(draft.window_opacity);
    }
  }, [draft?.window_opacity]);

  useEffect(() => {
    if (!isHotkeyEditing) {
      setHotkeyInputValue(draft?.global_hotkey ?? "");
    }
  }, [draft?.global_hotkey, isHotkeyEditing]);

  const updateDraftValue = useCallback(
    <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => {
      setDraft((prev) => (prev ? { ...prev, [key]: value } : prev));
    },
    [],
  );

  useEffect(() => {
    let unlistenResult: UnlistenFn | null = null;
    let unlistenCancel: UnlistenFn | null = null;
    let unlistenInvalid: UnlistenFn | null = null;

    const registerListeners = async () => {
      try {
        unlistenResult = await listen<HotkeyCapturePayload>(
          "hotkey_capture_result",
          (event) => {
            setIsCapturingHotkey(false);
            setIsHotkeyEditing(false);
            updateDraftValue("global_hotkey", event.payload.shortcut);
            setHotkeyInputValue(event.payload.shortcut);
            showToast(`快捷键已更新为 ${event.payload.shortcut}`);
          },
        );
        unlistenCancel = await listen("hotkey_capture_cancelled", () => {
          setIsCapturingHotkey(false);
          showToast("捕捉已取消");
        });
        unlistenInvalid = await listen("hotkey_capture_invalid", () => {
          showToast("请按下至少一个非修饰键或使用功能键");
        });
      } catch (error) {
        console.error("Failed to register hotkey capture listeners", error);
      }
    };

    void registerListeners();

    return () => {
      if (unlistenResult) {
        unlistenResult();
      }
      if (unlistenCancel) {
        unlistenCancel();
      }
      if (unlistenInvalid) {
        unlistenInvalid();
      }
    };
  }, [showToast, updateDraftValue]);

  useEffect(() => {
    return () => {
      void invoke("end_hotkey_capture");
    };
  }, []);

  const toggleBoolean = useCallback((key: BooleanSettingKey) => {
    setDraft((prev) => (prev ? { ...prev, [key]: !prev[key] } : prev));
  }, []);

  const handleOpacityChange = useCallback(
    (percent: number) => {
      const normalized = percent / 100;
      updateDraftValue("window_opacity", normalized);
      applyWindowOpacityVariable(normalized);
    },
    [updateDraftValue],
  );

  const isDirty = useMemo(() => {
    if (!settings || !draft) {
      return false;
    }
    return TRACKED_SETTING_KEYS.some((key) => settings[key] !== draft[key]);
  }, [settings, draft]);

  const validationMessage = useMemo(() => {
    if (!draft) {
      return "正在加载设置";
    }
    if (!draft.global_hotkey.trim()) {
      return "快捷键不能为空";
    }
    if (
      draft.query_delay_ms < MIN_QUERY_DELAY ||
      draft.query_delay_ms > MAX_QUERY_DELAY
    ) {
      return `延迟需在 ${MIN_QUERY_DELAY}~${MAX_QUERY_DELAY}ms 之间`;
    }
    if (
      draft.max_results < MIN_RESULT_LIMIT ||
      draft.max_results > MAX_RESULT_LIMIT
    ) {
      return `结果数量需在 ${MIN_RESULT_LIMIT}~${MAX_RESULT_LIMIT} 条之间`;
    }
    if (!draft.enable_app_results && !draft.enable_bookmark_results) {
      return "至少保留一个结果来源";
    }
    const validatePrefix = (value: string, label: string) => {
      if (!value) {
        return `${label} 前缀不能为空`;
      }
      const trimmedStart = value.replace(/^\s+/, "");
      if (!trimmedStart) {
        return `${label} 前缀不能为空`;
      }
      const firstChar = trimmedStart.charAt(0);
      if (!/^[a-zA-Z]$/.test(firstChar)) {
        return `${label} 前缀需为单个字母`;
      }
      const remainder = trimmedStart.slice(1);
      if (remainder && remainder !== " " && remainder !== ":") {
        return `${label} 前缀仅支持可选的空格或冒号结尾`;
      }
      return null;
    };
    const prefixError =
      validatePrefix(draft.prefix_app, "应用模式") ||
      validatePrefix(draft.prefix_bookmark, "书签模式") ||
      validatePrefix(draft.prefix_search, "搜索模式");
    if (prefixError) {
      return prefixError;
    }
    if (
      draft.window_opacity < MIN_WINDOW_OPACITY ||
      draft.window_opacity > MAX_WINDOW_OPACITY
    ) {
      return `透明度需在 ${MIN_WINDOW_OPACITY_PERCENT}%~${MAX_WINDOW_OPACITY_PERCENT}% 之间`;
    }
    return null;
  }, [draft]);

  const handleSettingsSave = useCallback(async () => {
    if (!draft) {
      return;
    }
    if (validationMessage) {
      showToast(validationMessage);
      return;
    }

    try {
      setIsSaving(true);
      const updated = await invoke<AppSettings>("update_settings", {
        updates: draft,
      });
      setSettings(updated);
      setDraft({ ...updated });
      showToast("设置已更新");
    } catch (error) {
      console.error("Failed to update settings", error);
      showToast("更新设置失败");
    } finally {
      setIsSaving(false);
    }
  }, [draft, showToast, validationMessage]);

  const normalizeHotkeyInput = useCallback((value: string) => {
    return value
      .split("+")
      .map((segment) => segment.trim())
      .filter(Boolean)
      .join("+");
  }, []);

  const handleHotkeyInputChange = useCallback(
    (event: ChangeEvent<HTMLInputElement>) => {
      if (isCapturingHotkey) {
        return;
      }
      const rawValue = event.currentTarget.value;
      setHotkeyInputValue(rawValue);
      const normalized = normalizeHotkeyInput(rawValue);
      updateDraftValue("global_hotkey", normalized);
    },
    [isCapturingHotkey, normalizeHotkeyInput, updateDraftValue],
  );

  const handleHotkeyInputKeyDown = useCallback(
    (event: InputKeyboardEvent<HTMLInputElement>) => {
      if (isCapturingHotkey) {
        event.preventDefault();
        return;
      }

      if (event.key === "Enter") {
        event.preventDefault();
        void handleSettingsSave();
      }
    },
    [handleSettingsSave, isCapturingHotkey],
  );

  const handleHotkeyInputBlur = useCallback(() => {
    setIsHotkeyEditing(false);
    if (isCapturingHotkey) {
      setIsCapturingHotkey(false);
      void invoke("end_hotkey_capture");
    }
    setHotkeyInputValue(draft?.global_hotkey ?? "");
  }, [draft?.global_hotkey, isCapturingHotkey]);

  const handleHotkeyInputFocus = useCallback(() => {
    setIsHotkeyEditing(true);
  }, []);

  const handleHotkeyCaptureButtonClick = useCallback(async () => {
    if (!draft) {
      return;
    }

    if (isCapturingHotkey) {
      setIsCapturingHotkey(false);
      showToast("已取消捕捉");
      await invoke("end_hotkey_capture");
      return;
    }

    try {
      await invoke("begin_hotkey_capture");
      setIsCapturingHotkey(true);
      showToast("正在监听：按组合键或按 Esc 取消");
      requestAnimationFrame(() => {
        if (hotkeyInputRef.current) {
          hotkeyInputRef.current.focus();
          hotkeyInputRef.current.select();
        }
      });
    } catch (error) {
      console.error("Failed to start hotkey capture", error);
      showToast("无法开始捕捉");
    }
  }, [draft, isCapturingHotkey, showToast]);

  const handleReset = useCallback(() => {
    if (settings) {
      setDraft({ ...settings });
      setActiveSection("general");
      setIsCapturingHotkey(false);
      setIsHotkeyEditing(false);
      setHotkeyInputValue(settings.global_hotkey);
      applyWindowOpacityVariable(settings.window_opacity);
      showToast("已恢复保存的配置");
      void invoke("end_hotkey_capture");
    }
  }, [settings, showToast]);

  const renderPlaceholder = () => (
    <div className="settings-loading">正在载入 Flow 风格设置...</div>
  );

  const renderGeneralSection = () => {
    if (!draft) {
      return renderPlaceholder();
    }

    return (
      <div className="settings-section">
        <article className="settings-card">
          <header className="settings-card__header">
            <div>
              <p className="settings-card__title">全局快捷键</p>
            </div>
            <span className="settings-chip">前台</span>
          </header>
          <div className="settings-input-row">
            <div className="settings-hotkey-control">
              <input
                ref={hotkeyInputRef}
                type="text"
                value={hotkeyInputValue}
                readOnly={isCapturingHotkey}
                onChange={handleHotkeyInputChange}
                onKeyDown={handleHotkeyInputKeyDown}
                onFocus={handleHotkeyInputFocus}
                onBlur={handleHotkeyInputBlur}
                className="settings-input"
                placeholder="点击右侧按钮以捕捉"
              />
              <button
                type="button"
                className={`ghost-button ghost-button--compact ${isCapturingHotkey ? "ghost-button--capturing" : ""}`}
                onClick={handleHotkeyCaptureButtonClick}
              >
                {isCapturingHotkey ? "停止捕捉" : "捕捉快捷键"}
              </button>
            </div>
            <span className="settings-hint">
              当前模式：{isCapturingHotkey ? "监听（系统级捕捉）" : "输入（可手动编辑文本）"}。
              {" "}
              {isCapturingHotkey
                ? "按下组合键或按 Esc 取消，期间主窗口热键会被暂时屏蔽"
                : "如需完全模仿 Flow Launcher 的捕捉，请点击右侧按钮进入监听模式"}
            </span>
          </div>
        </article>
        <article className="settings-card">
          <header className="settings-card__header">
            <div>
              <p className="settings-card__title">搜索防抖</p>
              <p className="settings-card__subtitle">
                避免过于频繁的调用，保持顺滑体验
              </p>
            </div>
            <span className="settings-chip">{draft.query_delay_ms} ms</span>
          </header>
          <div className="settings-slider">
            <input
              type="range"
              min={MIN_QUERY_DELAY}
              max={MAX_QUERY_DELAY}
              step={10}
              value={draft.query_delay_ms}
              onChange={(event: ChangeEvent<HTMLInputElement>) =>
                updateDraftValue(
                  "query_delay_ms",
                  Number(event.currentTarget.value),
                )
              }
            />
            <div className="settings-slider__scale">
              <span>{MIN_QUERY_DELAY}ms</span>
              <span>{MAX_QUERY_DELAY}ms</span>
            </div>
          </div>
          <div className="settings-number">
            <input
              type="number"
              min={MIN_QUERY_DELAY}
              max={MAX_QUERY_DELAY}
              step={10}
              value={draft.query_delay_ms}
              onChange={(event: ChangeEvent<HTMLInputElement>) =>
                updateDraftValue(
                  "query_delay_ms",
                  Number(event.currentTarget.value || draft.query_delay_ms),
                )
              }
            />
            <span className="settings-hint">
              范围 {MIN_QUERY_DELAY}~{MAX_QUERY_DELAY} ms
            </span>
          </div>
        </article>
        <article className="settings-card">
          <header className="settings-card__header">
            <div>
              <p className="settings-card__title">窗口透明度</p>
              <p className="settings-card__subtitle">
                越低越贴近 Flow Launcher 的玻璃质感
              </p>
            </div>
            <span className="settings-chip">
              {Math.round(draft.window_opacity * 100)}%
            </span>
          </header>
          <div className="settings-slider">
            <input
              type="range"
              min={MIN_WINDOW_OPACITY_PERCENT}
              max={MAX_WINDOW_OPACITY_PERCENT}
              step={1}
              value={Math.round(draft.window_opacity * 100)}
              onChange={(event: ChangeEvent<HTMLInputElement>) =>
                handleOpacityChange(Number(event.currentTarget.value))
              }
            />
            <div className="settings-slider__scale">
              <span>{MIN_WINDOW_OPACITY_PERCENT}%</span>
              <span>{MAX_WINDOW_OPACITY_PERCENT}%</span>
            </div>
          </div>
          <span className="settings-hint">
            调整后无需保存即可预览效果，保存以便下次沿用
          </span>
        </article>
        <article className="settings-card">
          <header className="settings-card__header">
            <div>
              <p className="settings-card__title">启动与输入法</p>
              <p className="settings-card__subtitle">
                控制开机自启及唤起时的输入法行为
              </p>
            </div>
          </header>
          <div className="settings-toggle-group">
            <button
              type="button"
              className={`settings-toggle ${draft.launch_on_startup ? "on" : "off"}`}
              onClick={() => toggleBoolean("launch_on_startup")}
            >
              <span className="toggle-pill" aria-hidden="true" />
              <div>
                <div className="toggle-title">开机自启动</div>
                <div className="toggle-subtitle">
                  登录 Windows 后自动运行 RustLauncher
                </div>
              </div>
            </button>
            <button
              type="button"
              className={`settings-toggle ${draft.force_english_input ? "on" : "off"}`}
              onClick={() => toggleBoolean("force_english_input")}
            >
              <span className="toggle-pill" aria-hidden="true" />
              <div>
                <div className="toggle-title">唤起后切换英文输入</div>
                <div className="toggle-subtitle">
                  确保搜索框默认使用英文符号/快捷键
                </div>
              </div>
            </button>
            <button
              type="button"
              className={`settings-toggle ${draft.debug_mode ? "on" : "off"}`}
              onClick={() => toggleBoolean("debug_mode")}
            >
              <span className="toggle-pill" aria-hidden="true" />
              <div>
                <div className="toggle-title">调试模式</div>
                <div className="toggle-subtitle">允许通过右键显示调试菜单</div>
              </div>
            </button>
          </div>
        </article>
      </div>
    );
  };

  const renderSearchSection = () => {
    if (!draft) {
      return renderPlaceholder();
    }

    return (
      <div className="settings-section">
        <article className="settings-card">
          <header className="settings-card__header">
            <div>
              <p className="settings-card__title">结果来源</p>
            </div>
          </header>
          <div className="settings-toggle-group">
            <button
              type="button"
              className={`settings-toggle ${draft.enable_app_results ? "on" : "off"}`}
              onClick={() => toggleBoolean("enable_app_results")}
            >
              <span className="toggle-pill" aria-hidden="true" />
              <div>
                <div className="toggle-title">包含应用</div>
                <div className="toggle-subtitle">检索 Win32 / UWP 程序</div>
              </div>
            </button>
            <button
              type="button"
              className={`settings-toggle ${draft.enable_bookmark_results ? "on" : "off"}`}
              onClick={() => toggleBoolean("enable_bookmark_results")}
            >
              <span className="toggle-pill" aria-hidden="true" />
              <div>
                <div className="toggle-title">包含书签</div>
                <div className="toggle-subtitle">同步 Chrome 收藏夹</div>
              </div>
            </button>
          </div>
        </article>
        <article className="settings-card">
          <header className="settings-card__header">
            <div>
              <p className="settings-card__title">结果数量上限</p>
              <p className="settings-card__subtitle">
                配合虚拟列表，最多 {MAX_RESULT_LIMIT} 条
              </p>
            </div>
            <span className="settings-chip">{draft.max_results} 条</span>
          </header>
          <div className="settings-slider">
            <input
              type="range"
              min={MIN_RESULT_LIMIT}
              max={MAX_RESULT_LIMIT}
              step={1}
              value={draft.max_results}
              onChange={(event: ChangeEvent<HTMLInputElement>) =>
                updateDraftValue(
                  "max_results",
                  Number(event.currentTarget.value),
                )
              }
            />
            <div className="settings-slider__scale">
              <span>{MIN_RESULT_LIMIT} 条</span>
              <span>{MAX_RESULT_LIMIT} 条</span>
            </div>
          </div>
        </article>
        <article className="settings-card">
          <header className="settings-card__header">
            <div>
              <p className="settings-card__title">模式前缀</p>
              <p className="settings-card__subtitle">
                自定义 a/b/s 风格的模式切换前缀
              </p>
            </div>
          </header>
          <div className="settings-prefix-grid">
            <div className="settings-prefix-row">
              <label className="settings-prefix-label" htmlFor="prefix_app">
                应用模式
              </label>
              <input
                id="prefix_app"
                type="text"
                maxLength={2}
                className="settings-input settings-input--small"
                value={draft.prefix_app}
                onChange={(event: ChangeEvent<HTMLInputElement>) =>
                  updateDraftValue("prefix_app", event.currentTarget.value)
                }
              />
              <span className="settings-hint">
                例如 "a " 或 "a:", 更贴近 Flow 的前缀体验
              </span>
            </div>
            <div className="settings-prefix-row">
              <label
                className="settings-prefix-label"
                htmlFor="prefix_bookmark"
              >
                书签模式
              </label>
              <input
                id="prefix_bookmark"
                type="text"
                maxLength={2}
                className="settings-input settings-input--small"
                value={draft.prefix_bookmark}
                onChange={(event: ChangeEvent<HTMLInputElement>) =>
                  updateDraftValue("prefix_bookmark", event.currentTarget.value)
                }
              />
              <span className="settings-hint">例如 "b "，在输入 b+空格 时切换</span>
            </div>
            <div className="settings-prefix-row">
              <label className="settings-prefix-label" htmlFor="prefix_search">
                搜索模式
              </label>
              <input
                id="prefix_search"
                type="text"
                maxLength={2}
                className="settings-input settings-input--small"
                value={draft.prefix_search}
                onChange={(event: ChangeEvent<HTMLInputElement>) =>
                  updateDraftValue("prefix_search", event.currentTarget.value)
                }
              />
              <span className="settings-hint">例如 "s"、"s:" 或 "s "</span>
            </div>
          </div>
        </article>
      </div>
    );
  };

  const renderAboutSection = () => {
    const summary = draft ?? settings;
    return (
      <div className="settings-section settings-section--grid">
        <div className="about-card">
          <div className="about-label">版本</div>
          <div className="about-value">{appVersion}</div>
        </div>
        <div className="about-card">
          <div className="about-label">快捷键</div>
          <div className="about-value">{summary?.global_hotkey ?? "--"}</div>
        </div>
        <div className="about-card">
          <div className="about-label">延迟</div>
          <div className="about-value">
            {summary ? `${summary.query_delay_ms} ms` : "--"}
          </div>
        </div>
        <div className="about-card">
          <div className="about-label">结果上限</div>
          <div className="about-value">
            {summary ? `${summary.max_results} 条` : "--"}
          </div>
        </div>
      </div>
    );
  };

  const renderSection = () => {
    switch (activeSection) {
      case "general":
        return renderGeneralSection();
      case "search":
        return renderSearchSection();
      case "about":
        return renderAboutSection();
      default:
        return null;
    }
  };

  return (
    <div className="settings-window">
      <header className="settings-window__header" data-tauri-drag-region>
        <div>
          <h1 className="settings-window__title">设置</h1>
          <p className="settings-window__subtitle">
            管理 RustLauncher 的快捷键、搜索与外观
          </p>
        </div>
      </header>
      <div className="settings-shell">
        <nav className="settings-sidebar">
          {SECTION_DEFS.map((section) => (
            <button
              key={section.id}
              type="button"
              className={`settings-nav__item ${activeSection === section.id ? "active" : ""}`}
              onClick={() => setActiveSection(section.id)}
            >
              <span className="settings-nav__icon" aria-hidden="true">
                {section.icon}
              </span>
              <span>
                <span className="settings-nav__label">{section.label}</span>
                <span className="settings-nav__desc">
                  {section.description}
                </span>
              </span>
            </button>
          ))}
        </nav>
        <section className="settings-panel">{renderSection()}</section>
      </div>
      <footer className="settings-window__footer">
        <div className="settings-footer__status">
          {validationMessage ?? (isDirty ? "有更改尚未保存" : "配置已同步")}
        </div>
        <div className="settings-footer__actions">
          <button type="button" className="ghost-button" onClick={handleReset}>
            恢复已保存
          </button>
          <button
            type="button"
            className="primary-button"
            onClick={() => void handleSettingsSave()}
            disabled={!isDirty || !!validationMessage || isSaving}
          >
            {isSaving ? "保存中..." : "保存更改"}
          </button>
        </div>
      </footer>
      {toastMessage ? <Toast message={toastMessage} /> : null}
    </div>
  );
};
