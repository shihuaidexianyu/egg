import type { ModeConfig, ModeId } from "../types";

export type ModeStripProps = {
    modes: ModeConfig[];
    activeModeId: ModeId;
    onSelect: (mode: ModeConfig) => void;
};

export const ModeStrip = ({ modes, activeModeId, onSelect }: ModeStripProps) => {
    return (
        <div className="mode-strip" role="radiogroup" aria-label="模式切换">
            {modes.map((mode) => (
                <button
                    key={mode.id}
                    type="button"
                    className={mode.id === activeModeId ? "mode-chip active" : "mode-chip"}
                    aria-pressed={mode.id === activeModeId}
                    onClick={() => onSelect(mode)}
                >
                    <span>{mode.label}</span>
                    {mode.prefix ? <kbd>{mode.prefix}</kbd> : <span className="chip-placeholder" />}
                </button>
            ))}
        </div>
    );
};
