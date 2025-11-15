import { useCallback, useEffect, useRef, useState } from "react";
import type { ChangeEvent, KeyboardEvent as InputKeyboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { AppSettings } from "../types";
import { Toast } from "./Toast";

export const SettingsWindow = () => {
    const [settings, setSettings] = useState<AppSettings | null>(null);
    const [hotkeyInput, setHotkeyInput] = useState("");
    const [queryDelayInput, setQueryDelayInput] = useState("");
    const [isSaving, setIsSaving] = useState(false);
    const [toastMessage, setToastMessage] = useState<string | null>(null);
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
            setHotkeyInput(appSettings.global_hotkey);
            setQueryDelayInput(String(appSettings.query_delay_ms));
        } catch (error) {
            console.error("Failed to load settings", error);
            showToast("加载设置失败");
        }
    }, [showToast]);

    useEffect(() => {
        void loadSettings();
        return () => {
            if (toastTimerRef.current) {
                window.clearTimeout(toastTimerRef.current);
            }
        };
    }, [loadSettings]);

    useEffect(() => {
        if (hotkeyInputRef.current) {
            hotkeyInputRef.current.focus();
            hotkeyInputRef.current.select();
        }
    }, [settings]);

    const handleClose = useCallback(async () => {
        const windowRef = getCurrentWindow();
        await windowRef.close();
    }, []);

    const handleSettingsSave = useCallback(async () => {
        const trimmedHotkey = hotkeyInput.trim();
        if (!trimmedHotkey) {
            showToast("快捷键不能为空");
            return;
        }

        const trimmedDelay = queryDelayInput.trim();
        if (!trimmedDelay) {
            showToast("延迟不能为空");
            return;
        }

        const parsedDelay = Number(trimmedDelay);
        if (!Number.isFinite(parsedDelay)) {
            showToast("请输入有效的延迟毫秒数");
            return;
        }

        if (parsedDelay < 50 || parsedDelay > 2000) {
            showToast("延迟需在 50~2000ms 之间");
            return;
        }

        try {
            setIsSaving(true);
            const updated = await invoke<AppSettings>("update_hotkey", {
                hotkey: trimmedHotkey,
                query_delay_ms: Math.round(parsedDelay),
            });
            setSettings(updated);
            setHotkeyInput(updated.global_hotkey);
            setQueryDelayInput(String(updated.query_delay_ms));
            showToast("设置已更新");
        } catch (error) {
            console.error("Failed to update settings", error);
            showToast("更新设置失败");
        } finally {
            setIsSaving(false);
        }
    }, [hotkeyInput, queryDelayInput, showToast]);

    const handleKeyDown = useCallback(
        (event: InputKeyboardEvent<HTMLInputElement>) => {
            if (event.key === "Enter") {
                event.preventDefault();
                void handleSettingsSave();
            }
        },
        [handleSettingsSave],
    );

    return (
        <div className="settings-window">
            <header className="settings-window__header" data-tauri-drag-region>
                <div>
                    <h1 className="settings-window__title">RustLauncher 设置</h1>
                    <p className="settings-window__subtitle">配置全局唤起与搜索体验</p>
                </div>
                <button type="button" className="ghost-button" onClick={() => void handleClose()}>
                    关闭
                </button>
            </header>
            <section className="settings-window__content">
                <div className="settings-field">
                    <label htmlFor="hotkey-input">全局快捷键</label>
                    <input
                        id="hotkey-input"
                        type="text"
                        ref={hotkeyInputRef}
                        value={hotkeyInput}
                        onChange={(event: ChangeEvent<HTMLInputElement>) =>
                            setHotkeyInput(event.currentTarget.value)
                        }
                        onKeyDown={handleKeyDown}
                        placeholder="例如 Alt+Space"
                        className="settings-input"
                    />
                    <span className="settings-hint">用 + 连接组合键，例如 Ctrl+Shift+P</span>
                </div>
                <div className="settings-field">
                    <label htmlFor="query-delay-input">匹配延迟 (毫秒)</label>
                    <input
                        id="query-delay-input"
                        type="number"
                        min={50}
                        max={2000}
                        step={10}
                        value={queryDelayInput}
                        onChange={(event: ChangeEvent<HTMLInputElement>) =>
                            setQueryDelayInput(event.currentTarget.value)
                        }
                        onKeyDown={handleKeyDown}
                        placeholder="例如 120"
                        className="settings-input"
                    />
                    <span className="settings-hint">控制搜索防抖延迟，范围 50~2000 毫秒</span>
                </div>
            </section>
            <footer className="settings-window__footer">
                <button type="button" className="ghost-button" onClick={() => void loadSettings()}>
                    重置
                </button>
                <button
                    type="button"
                    className="primary-button"
                    onClick={() => void handleSettingsSave()}
                    disabled={isSaving}
                >
                    {isSaving ? "保存中..." : "保存"}
                </button>
            </footer>
            {toastMessage ? <Toast message={toastMessage} /> : null}
            <dl className="settings-window__meta">
                <div>
                    <dt>当前快捷键</dt>
                    <dd>{settings?.global_hotkey ?? "加载中..."}</dd>
                </div>
                <div>
                    <dt>当前延迟</dt>
                    <dd>{settings ? `${settings.query_delay_ms} ms` : "加载中..."}</dd>
                </div>
            </dl>
        </div>
    );
};
