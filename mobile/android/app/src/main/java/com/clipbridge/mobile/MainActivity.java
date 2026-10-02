package com.clipbridge.mobile;

import android.os.Bundle;

import com.getcapacitor.BridgeActivity;

public class MainActivity extends BridgeActivity {
    @Override
    public void onCreate(Bundle savedInstanceState) {
        registerPlugin(ClipboardBridgePlugin.class);
        super.onCreate(savedInstanceState);
    }
}
