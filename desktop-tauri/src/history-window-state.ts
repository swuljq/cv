export const WINDOW_PINNED_STORAGE_KEY = 'clipbridge-window-pinned';

export const readWindowPinned = (value: string | null) => value === 'true';

export const shouldAutoHideHistory = (windowPinned: boolean) => !windowPinned;
