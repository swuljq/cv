package com.clipbridge.mobile;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.Service;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Intent;
import android.os.IBinder;
import android.text.TextUtils;

import androidx.annotation.Nullable;
import androidx.core.app.NotificationCompat;

import org.json.JSONObject;

import java.util.UUID;
import java.util.concurrent.TimeUnit;

import okhttp3.OkHttpClient;
import okhttp3.Request;
import okhttp3.Response;
import okhttp3.WebSocket;
import okhttp3.WebSocketListener;

public class ClipboardSyncService extends Service {
    private static final String CHANNEL_ID = "clipbridge_sync";
    private static final int NOTIFICATION_ID = 8787;
    private WebSocket socket;
    private ClipboardManager clipboard;
    private String lastText = "";
    private boolean stopping;

    @Override public void onCreate() {
        super.onCreate();
        createChannel();
        clipboard = (ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
        if (clipboard != null) clipboard.addPrimaryClipChangedListener(clipListener);
    }

    private final ClipboardManager.OnPrimaryClipChangedListener clipListener = this::sendClipboard;

    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        startForeground(NOTIFICATION_ID, notification("正在后台同步剪贴板"));
        if (intent != null && intent.hasExtra("url")) connect(intent);
        return START_STICKY;
    }

    private void connect(Intent intent) {
        String url = intent.getStringExtra("url");
        if (TextUtils.isEmpty(url)) return;
        if (socket != null) socket.close(1000, "reconnect");
        Request request = new Request.Builder().url(url).build();
        OkHttpClient client = new OkHttpClient.Builder().readTimeout(0, TimeUnit.MILLISECONDS).build();
        socket = client.newWebSocket(request, new WebSocketListener() {
            @Override public void onOpen(WebSocket ws, Response response) {
                try {
                    JSONObject auth = new JSONObject().put("type", "auth")
                        .put("username", intent.getStringExtra("user"))
                        .put("password", intent.getStringExtra("password"))
                        .put("deviceId", "android-service-" + UUID.randomUUID())
                        .put("deviceName", "Android 后台服务");
                    ws.send(auth.toString());
                } catch (Exception ignored) { }
            }
            @Override public void onMessage(WebSocket ws, String text) {
                try {
                    JSONObject message = new JSONObject(text);
                    if ("clipboard".equals(message.optString("type")) && "text".equals(message.optString("contentType"))) {
                        String value = message.optString("data", "");
                        lastText = value;
                        if (clipboard != null) clipboard.setPrimaryClip(ClipData.newPlainText("ClipBridge", value));
                    }
                } catch (Exception ignored) { }
            }
        });
    }

    private void sendClipboard() {
        if (stopping || socket == null || clipboard == null || !clipboard.hasPrimaryClip()) return;
        CharSequence value = clipboard.getPrimaryClip().getItemAt(0).coerceToText(this);
        String text = value == null ? "" : value.toString();
        if (text.isEmpty() || text.equals(lastText)) return;
        lastText = text;
        try { socket.send(new JSONObject().put("type", "clipboard").put("eventId", UUID.randomUUID().toString()).put("contentType", "text").put("data", text).toString()); } catch (Exception ignored) { }
    }

    private Notification notification(String text) { return new NotificationCompat.Builder(this, CHANNEL_ID).setContentTitle("ClipBridge").setContentText(text).setSmallIcon(android.R.drawable.stat_notify_sync).setOngoing(true).build(); }
    private void createChannel() { NotificationManager manager = getSystemService(NotificationManager.class); if (manager != null) manager.createNotificationChannel(new NotificationChannel(CHANNEL_ID, "剪贴板同步", NotificationManager.IMPORTANCE_LOW)); }
    @Override public void onDestroy() { stopping = true; if (clipboard != null) clipboard.removePrimaryClipChangedListener(clipListener); if (socket != null) socket.close(1000, "stop"); super.onDestroy(); }
    @Nullable @Override public IBinder onBind(Intent intent) { return null; }
}
