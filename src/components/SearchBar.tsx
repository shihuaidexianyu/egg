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
  rightContent?: React.ReactNode;
};

export const SearchBar = ({
  value,
  placeholder,
  inputRef,
  onChange,
  onCompositionStart,
  onCompositionEnd,
  onKeyDown,
  rightContent,
}: SearchBarProps) => {
  return (
    <div className="search-shell" data-testid="search-shell">
      <svg
        viewBox="0 0 1024 1024"
        className="search-icon"
        width="22"
        height="22"
        fill="currentColor"
        aria-hidden="true"
      >
        <path d="M448 768A320 320 0 1 0 448 128a320 320 0 0 0 0 640zm292.512-76.832l209.632 209.632c13.248 13.248 13.248 34.72 0 47.968-13.248 13.248-34.72 13.248-47.968 0l-209.632-209.632a382.208 382.208 0 0 1-244.544 88.864C236.288 828 64 655.712 64 444 64 232.288 236.288 60 448 60s384 172.288 384 384c0 91.808-32.512 176.416-87.488 244.16z" />
      </svg>
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
      {rightContent}
    </div>
  );
};
