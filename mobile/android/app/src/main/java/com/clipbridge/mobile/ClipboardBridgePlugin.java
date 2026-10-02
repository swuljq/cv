package com.clipbridge.mobile;

import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.provider.Settings;
import androidx.core.content.ContextCompat;

import com.getcapacitor.JSObject;
import com.getcapacitor.Plugin;
import com.getcapacitor.PluginCall;
import com.getcapacitor.PluginMethod;
import com.getcapacitor.annotation.CapacitorPlugin;

@CapacitorPlugin(name = "ClipboardBridge")
public class ClipboardBridgePlugin extends Plugin {
    @PluginMethod
    public void startSync(PluginCall call) {
        Intent intent = new Intent(getContext(), ClipboardSyncService.class)
            .putExtra("url", call.getString("url", ""))
            .putExtra("user", call.getString("user", ""))
            .putExtra("password", call.getString("password", ""));
        ContextCompat.startForegroundService(getContext(), intent);
        call.resolve();
    }

    @PluginMethod
    public void stopSync(PluginCall call) {
        getContext().stopService(new Intent(getContext(), ClipboardSyncService.class));
        call.resolve();
    }

    @PluginMethod
    public void openAccessibilitySettings(PluginCall call) {
        getContext().startActivity(new Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
        call.resolve();
    }
    @PluginMethod
    public void read(PluginCall call) {
        ClipboardManager manager = (ClipboardManager) getContext().getSystemService(Context.CLIPBOARD_SERVICE);
        JSObject result = new JSObject();
        if (manager == null || !manager.hasPrimaryClip()) {
            result.put("type", "text/plain");
            result.put("value", "");
            call.resolve(result);
            return;
        }
        ClipData data = manager.getPrimaryClip();
        CharSequence text = data != null && data.getItemCount() > 0 ? data.getItemAt(0).coerceToText(getContext()) : "";
        result.put("type", "text/plain");
        result.put("value", text == null ? "" : text.toString());
        call.resolve(result);
    }

    @PluginMethod
    public void write(PluginCall call) {
        String value = call.getString("string", "");
        ClipboardManager manager = (ClipboardManager) getContext().getSystemService(Context.CLIPBOARD_SERVICE);
        if (manager == null) {
            call.reject("Clipboard service unavailable");
            return;
        }
        manager.setPrimaryClip(ClipData.newPlainText("ClipBridge", value));
        call.resolve();
    }
}
