import type {
    ChangeEvent,
    CompositionEvent,
    KeyboardEvent as InputKeyboardEvent,
    RefObject,
} from "react";
import type { ModeConfig } from "../types";

export type SearchBarProps = {
    value: string;
    placeholder: string;
    activeMode: ModeConfig;
    inputRef: RefObject<HTMLInputElement>;
    onChange: (event: ChangeEvent<HTMLInputElement>) => void;
    onCompositionStart: (event: CompositionEvent<HTMLInputElement>) => void;
    onCompositionEnd: (event: CompositionEvent<HTMLInputElement>) => void;
    onKeyDown: (event: InputKeyboardEvent<HTMLInputElement>) => void;
};

export const SearchBar = ({
    value,
    placeholder,
    activeMode,
    inputRef,
    onChange,
    onCompositionStart,
    onCompositionEnd,
    onKeyDown,
}: SearchBarProps) => {
    const badgeClassName =
        activeMode.id === "all" ? "mode-badge" : `mode-badge mode-${activeMode.id}`;

    return (
        <div className="search-shell" data-testid="search-shell">
            <div className="search-icon" aria-hidden="true">
                ⌕
            </div>
            <div className={badgeClassName} aria-live="polite">
                {activeMode.label}
                {activeMode.prefix ? ` · ${activeMode.prefix}` : ""}
            </div>
            <input
                ref={inputRef}
                type="text"
                className="search-bar"
                value={value}
                onChange={onChange}
                onCompositionStart={onCompositionStart}
                onCompositionEnd={onCompositionEnd}
                onKeyDown={onKeyDown}
                placeholder={placeholder}
                autoFocus
                role="searchbox"
                aria-label="Flow 搜索输入"
            />
        </div>
    );
};
