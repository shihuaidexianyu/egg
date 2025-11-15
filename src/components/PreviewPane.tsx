import type { FallbackVisual, SearchResult } from "../types";

export type PreviewPaneProps = {
    result: SearchResult | null;
    fallbackVisual: FallbackVisual | null;
    tagLabel?: string | null;
    onPrev: () => void;
    onNext: () => void;
    onExecute: () => void;
    disableNavigation: boolean;
};

export const PreviewPane = ({
    result,
    fallbackVisual,
    tagLabel,
    onPrev,
    onNext,
    onExecute,
    disableNavigation,
}: PreviewPaneProps) => {
    if (!result) {
        return (
            <aside className="preview-panel muted">
                <div className="preview-placeholder">
                    <div className="preview-title">等待输入</div>
                    <div className="preview-subtitle">选择一条结果以查看详细信息</div>
                </div>
            </aside>
        );
    }

    return (
        <aside className="preview-panel">
            <div className="preview-card">
                {result.icon ? (
                    <img
                        src={`data:image/png;base64,${result.icon}`}
                        className="preview-icon"
                        alt={result.title}
                        draggable={false}
                    />
                ) : (
                    <div
                        className="preview-icon placeholder"
                        style={{
                            background: fallbackVisual?.background,
                            color: fallbackVisual?.color,
                        }}
                        aria-hidden="true"
                    >
                        {fallbackVisual?.glyph ?? "◎"}
                    </div>
                )}
                <div className="preview-text">
                    <div className="preview-title">{result.title}</div>
                    <div className="preview-subtitle">{result.subtitle}</div>
                    <div className="preview-meta">
                        <span className="preview-tag">{tagLabel ?? result.action_id}</span>
                        <span className="preview-score">Score {result.score}</span>
                    </div>
                </div>
                <div className="preview-actions">
                    <button
                        type="button"
                        className="ghost-button"
                        onClick={onPrev}
                        disabled={disableNavigation}
                    >
                        上一条
                    </button>
                    <button
                        type="button"
                        className="ghost-button"
                        onClick={onNext}
                        disabled={disableNavigation}
                    >
                        下一条
                    </button>
                    <button type="button" className="primary-button" onClick={onExecute}>
                        立即打开
                    </button>
                </div>
            </div>
        </aside>
    );
};
