import type {
    ChangeEvent,
    CompositionEvent,
    KeyboardEvent as InputKeyboardEvent,
    RefObject,
} from "react";

export type SearchBarProps = {
    value: string;
    placeholder: string;
    inputRef: RefObject<HTMLInputElement>;
    onChange: (event: ChangeEvent<HTMLInputElement>) => void;
    onCompositionStart: (event: CompositionEvent<HTMLInputElement>) => void;
    onCompositionEnd: (event: CompositionEvent<HTMLInputElement>) => void;
    onKeyDown: (event: InputKeyboardEvent<HTMLInputElement>) => void;
};

export const SearchBar = ({
    value,
    placeholder,
    inputRef,
    onChange,
    onCompositionStart,
    onCompositionEnd,
    onKeyDown,
}: SearchBarProps) => {
    return (
        <div className="search-shell" data-testid="search-shell">
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
