import { MODE_CONFIGS } from "../constants/modes";
import type { LauncherAction, LauncherState } from "../types";

export const initialLauncherState: LauncherState = {
    inputValue: "",
    searchQuery: "",
    results: [],
    selectedIndex: 0,
    toastMessage: null,
    settings: null,
    activeMode: MODE_CONFIGS.all,
    isModePrefixOnly: false,
    isComposing: false,
};

export const launcherReducer = (
    state: LauncherState,
    action: LauncherAction,
): LauncherState => {
    switch (action.type) {
        case "SET_INPUT":
            return {
                ...state,
                inputValue: action.payload.inputValue,
                searchQuery: action.payload.searchQuery,
                activeMode: action.payload.activeMode,
                isModePrefixOnly: action.payload.isModePrefixOnly,
            };
        case "SET_RESULTS":
            return {
                ...state,
                results: action.payload,
            };
        case "SET_SELECTED_INDEX":
            return {
                ...state,
                selectedIndex: action.payload,
            };
        case "SET_TOAST":
            return {
                ...state,
                toastMessage: action.payload,
            };
        case "SET_SETTINGS":
            return {
                ...state,
                settings: action.payload,
            };
        case "SET_COMPOSING":
            return {
                ...state,
                isComposing: action.payload,
            };
        case "RESET_SEARCH":
            return {
                ...state,
                inputValue: "",
                searchQuery: "",
                results: [],
                selectedIndex: 0,
                activeMode: MODE_CONFIGS.all,
                isModePrefixOnly: false,
            };
        default:
            return state;
    }
};
