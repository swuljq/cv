package com.clipbridge.mobile;

import android.accessibilityservice.AccessibilityService;
import android.content.ClipboardManager;
import android.util.Log;
import android.os.Handler;
import android.os.Looper;
import android.view.accessibility.AccessibilityEvent;

public class ClipboardAccessibilityService extends AccessibilityService {
    private ClipboardManager clipboard;
    private String lastText = "";
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final ClipboardManager.OnPrimaryClipChangedListener listener = this::onClipboardChanged;

    @Override protected void onServiceConnected() {
        super.onServiceConnected();
        clipboard = (ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
        if (clipboard != null) clipboard.addPrimaryClipChangedListener(listener);
        handler.post(poller);
        Log.i("ClipBridge", "无障碍剪贴板服务已启用");
    }

    private final Runnable poller = new Runnable() {
        @Override public void run() {
            onClipboardChanged();
            handler.postDelayed(this, 500);
        }
    };

    private void onClipboardChanged() {
        if (clipboard == null || !clipboard.hasPrimaryClip()) return;
        CharSequence value = clipboard.getPrimaryClip().getItemAt(0).coerceToText(this);
        String text = value == null ? "" : value.toString();
        if (!text.isEmpty() && !text.equals(lastText)) {
            lastText = text;
            Log.i("ClipBridge", "无障碍服务读取到手机剪贴板");
            ClipboardSyncService.sendFromAccessibility(text);
        }
    }

    @Override public void onAccessibilityEvent(AccessibilityEvent event) { }
    @Override public void onInterrupt() { }
    @Override public void onDestroy() { handler.removeCallbacks(poller); if (clipboard != null) clipboard.removePrimaryClipChangedListener(listener); super.onDestroy(); }
}
