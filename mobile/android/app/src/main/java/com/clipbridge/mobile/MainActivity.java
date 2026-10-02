package com.clipbridge.mobile;

import com.getcapacitor.BridgeActivity;

public class MainActivity extends BridgeActivity {
    @Override
    protected void load() {
        registerPlugin(ClipboardBridgePlugin.class);
        super.load();
    }
}
