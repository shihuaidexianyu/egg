import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ChangeEvent, KeyboardEvent as InputKeyboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import type { AppSettings } from "../types";
import { Toast } from "./Toast";

const MIN_QUERY_DELAY = 50;
const MAX_QUERY_DELAY = 2000;
const MIN_RESULT_LIMIT = 10;
const MAX_RESULT_LIMIT = 60;

type SettingsSectionId = "general" | "search" | "about";

const SECTION_DEFS: Array<{ id: SettingsSectionId; label: string; description: string; icon: string }> = [
    { id: "general", label: "常规", description: "呼出快捷键 & 防抖", icon: "⌘" },
    { id: "search", label: "搜索", description: "结果来源 / 数量", icon: "🔍" },
    { id: "about", label: "关于", description: "版本与状态", icon: "ℹ️" },
];

type BooleanSettingKey = "enable_app_results" | "enable_bookmark_results";

const TRACKED_SETTING_KEYS: Array<keyof AppSettings> = [
    "global_hotkey",
    "query_delay_ms",
    "max_results",
    "enable_app_results",
    "enable_bookmark_results",
    "prefix_app",
    "prefix_bookmark",
    "prefix_search",
];

export const SettingsWindow = () => {
    const [settings, setSettings] = useState<AppSettings | null>(null);
    const [draft, setDraft] = useState<AppSettings | null>(null);
    const [activeSection, setActiveSection] = useState<SettingsSectionId>("general");
    const [isSaving, setIsSaving] = useState(false);
    const [toastMessage, setToastMessage] = useState<string | null>(null);
    const [appVersion, setAppVersion] = useState("--");
    const hotkeyInputRef = useRef<HTMLInputElement | null>(null);
    const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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
        if (hotkeyInputRef.current && draft) {
            hotkeyInputRef.current.focus();
            hotkeyInputRef.current.select();
        }
    }, [draft]);

    const updateDraftValue = useCallback(<K extends keyof AppSettings>(key: K, value: AppSettings[K]) => {
        setDraft((prev) => (prev ? { ...prev, [key]: value } : prev));
    }, []);

    const toggleBoolean = useCallback((key: BooleanSettingKey) => {
        setDraft((prev) => (prev ? { ...prev, [key]: !prev[key] } : prev));
    }, []);

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
        if (draft.query_delay_ms < MIN_QUERY_DELAY || draft.query_delay_ms > MAX_QUERY_DELAY) {
            return `延迟需在 ${MIN_QUERY_DELAY}~${MAX_QUERY_DELAY}ms 之间`;
        }
        if (draft.max_results < MIN_RESULT_LIMIT || draft.max_results > MAX_RESULT_LIMIT) {
            return `结果数量需在 ${MIN_RESULT_LIMIT}~${MAX_RESULT_LIMIT} 条之间`;
        }
        if (!draft.enable_app_results && !draft.enable_bookmark_results) {
            return "至少保留一个结果来源";
        }
        const validatePrefix = (value: string, label: string) => {
            const trimmed = value.trim();
            if (!trimmed) {
                return `${label} 前缀不能为空`;
            }
            if (!/^[a-zA-Z]$/.test(trimmed)) {
                return `${label} 前缀需为单个字母`;
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
            const updated = await invoke<AppSettings>("update_settings", { updates: draft });
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

    const handleKeyDown = useCallback(
        (event: InputKeyboardEvent<HTMLInputElement>) => {
            if (event.key === "Enter") {
                event.preventDefault();
                void handleSettingsSave();
            }
        },
        [handleSettingsSave],
    );

    const handleReset = useCallback(() => {
        if (settings) {
            setDraft({ ...settings });
            setActiveSection("general");
            showToast("已恢复保存的配置");
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
                        <input
                            ref={hotkeyInputRef}
                            type="text"
                            value={draft.global_hotkey}
                            onChange={(event: ChangeEvent<HTMLInputElement>) =>
                                updateDraftValue("global_hotkey", event.currentTarget.value)
                            }
                            onKeyDown={handleKeyDown}
                            className="settings-input"
                            placeholder="例如 Alt+Space"
                        />
                        <span className="settings-hint">用 "+" 连接组合键，如 Ctrl+Shift+P</span>
                    </div>
                </article>
                <article className="settings-card">
                    <header className="settings-card__header">
                        <div>
                            <p className="settings-card__title">搜索防抖</p>
                            <p className="settings-card__subtitle">避免过于频繁的调用，保持顺滑体验</p>
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
                                updateDraftValue("query_delay_ms", Number(event.currentTarget.value))
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
                        <span className="settings-hint">范围 {MIN_QUERY_DELAY}~{MAX_QUERY_DELAY} ms</span>
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
                            <p className="settings-card__subtitle">配合虚拟列表，最多 {MAX_RESULT_LIMIT} 条</p>
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
                                updateDraftValue("max_results", Number(event.currentTarget.value))
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
                            <p className="settings-card__subtitle">自定义 a/b/s 风格的模式切换前缀</p>
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
                                maxLength={1}
                                className="settings-input settings-input--small"
                                value={draft.prefix_app}
                                onChange={(event: ChangeEvent<HTMLInputElement>) =>
                                    updateDraftValue("prefix_app", event.currentTarget.value)
                                }
                            />
                            <span className="settings-hint">例如 "a"，输入 a 空格 即切换</span>
                        </div>
                        <div className="settings-prefix-row">
                            <label className="settings-prefix-label" htmlFor="prefix_bookmark">
                                书签模式
                            </label>
                            <input
                                id="prefix_bookmark"
                                type="text"
                                maxLength={1}
                                className="settings-input settings-input--small"
                                value={draft.prefix_bookmark}
                                onChange={(event: ChangeEvent<HTMLInputElement>) =>
                                    updateDraftValue("prefix_bookmark", event.currentTarget.value)
                                }
                            />
                            <span className="settings-hint">例如 "b"</span>
                        </div>
                        <div className="settings-prefix-row">
                            <label className="settings-prefix-label" htmlFor="prefix_search">
                                搜索模式
                            </label>
                            <input
                                id="prefix_search"
                                type="text"
                                maxLength={1}
                                className="settings-input settings-input--small"
                                value={draft.prefix_search}
                                onChange={(event: ChangeEvent<HTMLInputElement>) =>
                                    updateDraftValue("prefix_search", event.currentTarget.value)
                                }
                            />
                            <span className="settings-hint">例如 "s"</span>
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
                    <div className="about-value">{summary ? `${summary.query_delay_ms} ms` : "--"}</div>
                </div>
                <div className="about-card">
                    <div className="about-label">结果上限</div>
                    <div className="about-value">{summary ? `${summary.max_results} 条` : "--"}</div>
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
                    <p className="settings-window__subtitle">管理 RustLauncher 的快捷键、搜索与外观</p>
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
                                <span className="settings-nav__desc">{section.description}</span>
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
