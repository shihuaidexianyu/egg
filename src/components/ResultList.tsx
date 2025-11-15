import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { UIEvent } from "react";
import type { SearchResult } from "../types";
import { pickFallbackIcon } from "../utils/fallbackIcon";

const ITEM_HEIGHT = 82;
const DEFAULT_VIEWPORT_ROWS = 8;

export type ResultListProps = {
    results: SearchResult[];
    selectedIndex: number;
    onSelect: (index: number) => void;
    onActivate: (item: SearchResult) => void;
    resolveResultTag: (item: SearchResult) => string;
};

export const ResultList = ({
    results,
    selectedIndex,
    onSelect,
    onActivate,
    resolveResultTag,
}: ResultListProps) => {
    if (results.length === 0) {
        return null;
    }

    const viewportRef = useRef<HTMLDivElement | null>(null);
    const [scrollOffset, setScrollOffset] = useState(0);
    const [viewportHeight, setViewportHeight] = useState(
        DEFAULT_VIEWPORT_ROWS * ITEM_HEIGHT,
    );

    useEffect(() => {
        const element = viewportRef.current;
        if (!element) {
            return;
        }

        const updateHeight = () => {
            setViewportHeight(element.clientHeight || DEFAULT_VIEWPORT_ROWS * ITEM_HEIGHT);
        };

        updateHeight();

        if (typeof ResizeObserver === "undefined") {
            return;
        }

        const observer = new ResizeObserver(() => {
            updateHeight();
        });

        observer.observe(element);

        return () => {
            observer.disconnect();
        };
    }, []);

    useEffect(() => {
        const maxOffset = Math.max(0, results.length * ITEM_HEIGHT - viewportHeight);
        if (scrollOffset > maxOffset) {
            setScrollOffset(0);
            if (viewportRef.current) {
                viewportRef.current.scrollTop = 0;
            }
        }
    }, [results.length, viewportHeight, scrollOffset]);

    const handleScroll = useCallback((event: UIEvent<HTMLDivElement>) => {
        setScrollOffset(event.currentTarget.scrollTop);
    }, []);

    const { items, offsetY, totalHeight } = useMemo(() => {
        const visibleRows = Math.max(1, Math.ceil(viewportHeight / ITEM_HEIGHT) + 1);
        const startIndex = Math.max(0, Math.floor(scrollOffset / ITEM_HEIGHT));
        const endIndex = Math.min(results.length, startIndex + visibleRows);
        const slice: Array<{ item: SearchResult; index: number }> = [];
        for (let index = startIndex; index < endIndex; index += 1) {
            slice.push({ item: results[index], index });
        }
        return {
            items: slice,
            offsetY: startIndex * ITEM_HEIGHT,
            totalHeight: results.length * ITEM_HEIGHT,
        };
    }, [results, scrollOffset, viewportHeight]);

    return (
        <div
            ref={viewportRef}
            className="result-virtual-wrapper"
            role="listbox"
            aria-activedescendant={results[selectedIndex]?.id}
            onScroll={handleScroll}
        >
            <div style={{ position: "relative", height: totalHeight, width: "100%" }}>
                <div style={{ position: "absolute", top: offsetY, left: 0, right: 0 }}>
                    {items.map(({ item, index }) => {
                        const isActive = index === selectedIndex;
                        const visual = pickFallbackIcon(item);
                        return (
                            <div
                                key={item.id}
                                className={isActive ? "result-item active" : "result-item"}
                                role="option"
                                aria-selected={isActive}
                                data-result-id={item.id}
                            >
                                <button
                                    type="button"
                                    className="result-button"
                                    onClick={() => onSelect(index)}
                                    onDoubleClick={() => onActivate(item)}
                                    onMouseEnter={() => onSelect(index)}
                                >
                                    {item.icon ? (
                                        <img
                                            src={`data:image/png;base64,${item.icon}`}
                                            className="result-icon"
                                            alt="result icon"
                                        />
                                    ) : (
                                        <div
                                            className="result-icon placeholder"
                                            style={{
                                                background: visual.background,
                                                color: visual.color,
                                            }}
                                        >
                                            {visual.glyph}
                                        </div>
                                    )}
                                    <div className="result-meta">
                                        <div className="result-title-row">
                                            <span className="result-title">{item.title}</span>
                                            <span className="result-tag">{resolveResultTag(item)}</span>
                                        </div>
                                        <div className="result-subtitle" title={item.subtitle}>
                                            {item.subtitle}
                                        </div>
                                    </div>
                                    <div className="result-shortcut" aria-hidden="true">
                                        {String(index + 1).padStart(2, "0")}
                                    </div>
                                </button>
                            </div>
                        );
                    })}
                </div>
            </div>
        </div>
    );
};
