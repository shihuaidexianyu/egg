import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  SETTINGS_UPDATED_EVENT,
  WINDOW_OPACITY_PREVIEW_EVENT,
} from "../constants/events";
import type { AppSettings } from "../types";
import { applyWindowOpacityVariable } from "../utils/theme";

const TABS = [
  { id: "general", label: "常规", icon: "⚙️", desc: "通用行为设置" },
  { id: "search", label: "搜索", icon: "🔍", desc: "搜索模式前缀" },
  { id: "appearance", label: "外观", icon: "🎨", desc: "窗口透明度" },
  { id: "about", label: "关于", icon: "ℹ️", desc: "版本信息" },
] as const;

type TabId = (typeof TABS)[number]["id"];

export const SettingsWindow = () => {
  const [activeTab, setActiveTab] = useState<TabId>("general");
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [loading, setLoading] = useState(true);

  const loadSettings = useCallback(async () => {
    try {
      const appSettings = await invoke<AppSettings>("get_settings");
      setSettings(appSettings);
    } catch (error) {
      console.error("Failed to load settings", error);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadSettings();
  }, [loadSettings]);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;

    const register = async () => {
      unlisten = await listen<AppSettings>(SETTINGS_UPDATED_EVENT, (event) => {
        setSettings(event.payload);
      });
    };

    void register();

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

  const updateSetting = useCallback(
    async (key: keyof AppSettings, value: any) => {
      if (!settings) {
        return;
      }
      const newSettings = { ...settings, [key]: value };
      setSettings(newSettings);
      try {
        await invoke("update_settings", { settings: newSettings });
      } catch (error) {
        console.error("Failed to update settings", error);
      }
    },
    [settings],
  );

  const previewOpacity = useCallback((value: number) => {
    applyWindowOpacityVariable(value);
    void invoke("emit", {
      event: WINDOW_OPACITY_PREVIEW_EVENT,
      payload: { value, temporary: true },
    });
  }, []);

  const commitOpacity = useCallback(
    (value: number) => {
      void updateSetting("window_opacity", value);
      void invoke("emit", {
        event: WINDOW_OPACITY_PREVIEW_EVENT,
        payload: { value, temporary: false },
      });
    },
    [updateSetting],
  );

  const handlePrefixChange = useCallback(
    (key: keyof AppSettings, newPrefix: string) => {
      if (!settings) {
        return;
      }
      const trimmed = newPrefix.trim();
      void updateSetting(key, trimmed);
    },
    [settings, updateSetting],
  );

  if (loading) {
    return <div className="settings-loading">正在加载设置...</div>;
  }

  return (
    <div className="settings-window">
      <div className="settings-window__header">
        <div>
          <h1 className="settings-window__title">设置</h1>
          <p className="settings-window__subtitle">
            配置 egg 的行为与外观
          </p>
        </div>
      </div>

      <div className="settings-shell">
        <nav className="settings-sidebar">
          {TABS.map((tab) => (
            <button
              key={tab.id}
              className={`settings-nav__item ${activeTab === tab.id ? "active" : ""}`}
              onClick={() => setActiveTab(tab.id)}
            >
              <span className="settings-nav__icon">{tab.icon}</span>
              <div className="settings-nav__content">
                <span className="settings-nav__label">{tab.label}</span>
                <span className="settings-nav__desc">{tab.desc}</span>
              </div>
            </button>
          ))}
        </nav>

        <main className="settings-panel">
          {activeTab === "general" && (
            <div className="settings-section">
              <div className="settings-card">
                <div className="settings-card__header">
                  <div>
                    <h3 className="settings-card__title">调试</h3>
                  </div>
                </div>
                <label
                  className={`settings-toggle ${settings?.debug_mode ? "on" : ""}`}
                >
                  <input
                    type="checkbox"
                    checked={settings?.debug_mode ?? false}
                    onChange={(e) =>
                      updateSetting("debug_mode", e.target.checked)
                    }
                    hidden
                  />
                  <div className="toggle-pill" />
                  <div>
                    <div className="toggle-title">调试模式</div>
                    <div className="toggle-subtitle">
                      启用右键菜单和开发者工具
                    </div>
                  </div>
                </label>
              </div>
            </div>
          )}

          {activeTab === "search" && (
            <div className="settings-section">
              <div className="settings-card">
                <div className="settings-card__header">
                  <div>
                    <h3 className="settings-card__title">搜索模式前缀</h3>
                    <p className="settings-card__subtitle">
                      自定义触发特定搜索模式的关键词
                    </p>
                  </div>
                </div>
                <div className="settings-prefix-grid">
                  {[
                    {
                      key: "prefix_app" as keyof AppSettings,
                      label: "应用搜索",
                      value: settings?.prefix_app,
                      default: "r",
                    },
                    {
                      key: "prefix_bookmark" as keyof AppSettings,
                      label: "书签搜索",
                      value: settings?.prefix_bookmark,
                      default: "b",
                    },
                    {
                      key: "prefix_search" as keyof AppSettings,
                      label: "网页搜索",
                      value: settings?.prefix_search,
                      default: "s",
                    },
                  ].map((item) => (
                    <div key={item.key} className="settings-prefix-row">
                      <span className="settings-prefix-label">
                        {item.label}
                      </span>
                      <input
                        type="text"
                        className="settings-input settings-input--small"
                        value={item.value ?? item.default}
                        onChange={(e) =>
                          handlePrefixChange(item.key, e.target.value)
                        }
                      />
                      <span className="settings-hint">
                        默认: {item.default}
                      </span>
                    </div>
                  ))}
                </div>
              </div>

              <div className="settings-card">
                <div className="settings-card__header">
                  <div>
                    <h3 className="settings-card__title">响应速度</h3>
                  </div>
                </div>
                <div className="settings-input-row">
                  <div className="settings-number">
                    <label>搜索延迟 (ms)</label>
                    <input
                      type="number"
                      value={settings?.query_delay_ms ?? 120}
                      onChange={(e) =>
                        updateSetting(
                          "query_delay_ms",
                          parseInt(e.target.value) || 0,
                        )
                      }
                    />
                  </div>
                  <p className="settings-hint">
                    输入停止后多久开始搜索，数值越小响应越快，但可能增加资源消耗
                  </p>
                </div>
              </div>
            </div>
          )}

          {activeTab === "appearance" && (
            <div className="settings-section">
              <div className="settings-card">
                <div className="settings-card__header">
                  <div>
                    <h3 className="settings-card__title">窗口透明度</h3>
                  </div>
                  <span className="settings-chip">
                    {Math.round((settings?.window_opacity ?? 1) * 100)}%
                  </span>
                </div>
                <div className="settings-slider">
                  <input
                    type="range"
                    min="0.1"
                    max="1"
                    step="0.01"
                    value={settings?.window_opacity ?? 1}
                    onInput={(e) =>
                      previewOpacity(parseFloat(e.currentTarget.value))
                    }
                    onChange={(e) =>
                      commitOpacity(parseFloat(e.currentTarget.value))
                    }
                  />
                  <div className="settings-slider__scale">
                    <span>透明</span>
                    <span>不透明</span>
                  </div>
                </div>
              </div>
            </div>
          )}

          {activeTab === "about" && (
            <div className="settings-section">
              <div className="about-card">
                <div className="about-label">当前版本</div>
                <div className="about-value">v0.1.0</div>
              </div>
              <div className="about-card">
                <div className="about-label">关于 egg</div>
                <p style={{ margin: "8px 0 0", lineHeight: "1.6" }}>
                  egg 是一个极简、高性能的现代化启动器，旨在提升您的工作效率。
                </p>
              </div>
            </div>
          )}
        </main>
      </div>

      <footer className="settings-window__footer">
        <div className="settings-footer__status">
          {loading ? "正在同步..." : "设置已保存"}
        </div>
        <div className="settings-footer__actions">
          <button
            className="ghost-button"
            onClick={() => invoke("open_config_dir")}
          >
            打开配置文件夹
          </button>
        </div>
      </footer>
    </div>
  );
};
