package com.clipbridge.mobile;

import android.os.Bundle;

import com.getcapacitor.BridgeActivity;
import com.capacitorjs.plugins.clipboard.ClipboardPlugin;

public class MainActivity extends BridgeActivity {
    @Override
    public void onCreate(Bundle savedInstanceState) {
        registerPlugin(ClipboardPlugin.class);
        super.onCreate(savedInstanceState);
    }
}
