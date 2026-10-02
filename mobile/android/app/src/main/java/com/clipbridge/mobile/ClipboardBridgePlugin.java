package com.clipbridge.mobile;

import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;

import com.getcapacitor.JSObject;
import com.getcapacitor.Plugin;
import com.getcapacitor.PluginCall;
import com.getcapacitor.PluginMethod;
import com.getcapacitor.annotation.CapacitorPlugin;

@CapacitorPlugin(name = "ClipboardBridge")
public class ClipboardBridgePlugin extends Plugin {
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
