package dev.wunder.android;

import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.PixelFormat;
import android.net.Uri;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.provider.Settings;
import android.util.Base64;
import android.util.DisplayMetrics;
import android.util.Log;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowManager;
import android.webkit.CookieManager;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.TextView;

import org.json.JSONObject;
import org.json.JSONTokener;

import java.io.ByteArrayOutputStream;
import java.util.Locale;
import java.util.concurrent.Callable;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.FutureTask;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;

/**
 * Process-scoped browser backed by the phone's system WebView.
 *
 * <p>The agent drives one floating browser window through a small action
 * contract. Nothing is packaged: Android already ships and updates the
 * Chromium-based WebView. All WebView and window operations happen on the main
 * thread; the engine calls `execute` from a blocking worker.
 */
public final class WebViewBridge {
    private static final String TAG = "WunderWebView";
    private static final long NAVIGATION_TIMEOUT_MS = 30_000L;
    private static final long JS_TIMEOUT_MS = 15_000L;
    private static final int MAX_TEXT_CHARS = 40_000;
    private static final int MAX_SNAPSHOT_NODES = 220;

    private static final Handler MAIN = new Handler(Looper.getMainLooper());

    private static Context app;
    private static WindowManager windowManager;
    private static WebView webView;
    private static View overlay;
    private static TextView titleView;
    private static volatile CountDownLatch pageLatch;
    private static volatile String currentUrl = "";
    private static volatile String currentTitle = "";
    private static volatile String lastLoadError = "";
    private static volatile long lastPermissionRequestAt;

    private WebViewBridge() {}

    public static void bind(Context context) {
        if (context != null) {
            app = context.getApplicationContext();
        }
    }

    /** Entry point used by the Rust browser provider. */
    public static String execute(Context context, String action, String argsJson) {
        try {
            if (context != null) {
                app = context.getApplicationContext();
            }
            if (app == null) {
                return error("应用上下文不可用");
            }
            JSONObject args = new JSONObject(argsJson == null || argsJson.isEmpty() ? "{}" : argsJson);
            String name = action == null ? "" : action.trim().toLowerCase(Locale.ROOT);
            switch (name) {
                case "navigate":
                    return navigate(args);
                case "back":
                    return back();
                case "observe_refs":
                    return snapshot();
                case "observe_text":
                    return readPage(args);
                case "read_page":
                    return readPage(args);
                case "click":
                    return click(args);
                case "type":
                    return type(args);
                case "press":
                    return press(args);
                case "hover":
                    return hover(args);
                case "select":
                    return select(args);
                case "drag":
                    return drag(args);
                case "wait":
                    return waitFor(args);
                case "screenshot":
                    return screenshot();
                case "tabs_list":
                    return tabsList();
                case "tabs_new":
                    return navigate(args);
                case "tabs_select":
                    return error("WebView 浏览器只支持一个标签页");
                case "tabs_close":
                    if (args.has("target_id") || args.has("index")) {
                        return error("WebView 浏览器只支持一个标签页");
                    }
                    return close();
                case "close":
                    return close();
                case "status":
                    return status();
                case "permission":
                    requestOverlayPermission();
                    return ok("已打开悬浮窗授权页面");
                default:
                    return error("不支持的动作：" + name);
            }
        } catch (Throwable error) {
            Log.w(TAG, "browser action failed: " + action, error);
            return error(message(error));
        }
    }

    private static String navigate(JSONObject args) {
        String url = args.optString("url", "").trim();
        if (!isHttpUrl(url)) {
            return error("url 必须是 http(s) 地址");
        }
        try {
            ensureBrowser();
        } catch (RuntimeException error) {
            return error(message(error));
        }
        CountDownLatch latch = new CountDownLatch(1);
        pageLatch = latch;
        lastLoadError = "";
        Boolean started = onMain(() -> {
            if (webView == null) {
                return Boolean.FALSE;
            }
            webView.loadUrl(url);
            return Boolean.TRUE;
        }, 8_000L);
        if (!Boolean.TRUE.equals(started)) {
            return error("无法启动浏览器窗口");
        }
        await(latch, NAVIGATION_TIMEOUT_MS);
        sleep(350);
        return pageText("已打开 " + (currentUrl.isEmpty() ? url : currentUrl));
    }

    private static String back() {
        if (webView == null) {
            return error("浏览器尚未打开");
        }
        CountDownLatch latch = new CountDownLatch(1);
        pageLatch = latch;
        Boolean moved = onMain(() -> {
            if (webView == null || !webView.canGoBack()) {
                return Boolean.FALSE;
            }
            webView.goBack();
            return Boolean.TRUE;
        }, 8_000L);
        if (!Boolean.TRUE.equals(moved)) {
            return error("没有可返回的上一页");
        }
        await(latch, 10_000L);
        sleep(250);
        return pageText("已返回 " + currentUrl);
    }

    private static String readPage(JSONObject args) {
        if (!ensureReady()) {
            return error("浏览器尚未打开");
        }
        String text = evalJs(
                "(function(){var b=document.body;return b?(b.innerText||b.textContent||''):'';})()",
                JS_TIMEOUT_MS);
        if (text == null) {
            return error("读取页面超时");
        }
        int limit = clamp(args.optInt("max_chars", MAX_TEXT_CHARS), 1, MAX_TEXT_CHARS);
        return pageText(truncate(text.trim(), limit));
    }

    private static String snapshot() {
        if (!ensureReady()) {
            return error("浏览器尚未打开");
        }
        String raw = evalJs(SNAPSHOT_JS, JS_TIMEOUT_MS);
        if (raw == null || raw.trim().isEmpty()) {
            return error("页面快照超时");
        }
        try {
            JSONObject data = new JSONObject(raw);
            String lines = data.optString("text", "");
            String header = data.optString("title", "") + "\n" + data.optString("url", "");
            return pageText(header + "\n" + lines);
        } catch (Throwable error) {
            return pageText(raw);
        }
    }

    private static String click(JSONObject args) {
        if (!ensureReady()) {
            return error("浏览器尚未打开");
        }
        String lookup = lookupExpression(args);
        if (lookup == null) {
            return error("click 需要 ref 或 selector");
        }
        boolean doubleClick = args.optBoolean("double_click", false);
        String script = "(function(){"
                + "var el=" + lookup + ";"
                + "if(!el)return 'not_found';"
                + "try{el.scrollIntoView({block:'center',inline:'center'});}catch(e){}"
                + "var r=el.getBoundingClientRect();"
                + "var x=r.left+r.width/2,y=r.top+r.height/2;"
                + "['pointerover','mouseover','pointerdown','mousedown','pointerup','mouseup']"
                + ".forEach(function(t){try{el.dispatchEvent(new MouseEvent(t,{bubbles:true,cancelable:true,clientX:x,clientY:y}));}catch(e){}});"
                + "try{el.click();}catch(e){}"
                + (doubleClick
                    ? "try{el.dispatchEvent(new MouseEvent('dblclick',{bubbles:true,cancelable:true,clientX:x,clientY:y}));}catch(e){}"
                    : "")
                + "return 'clicked';})()";
        String result = evalJs(script, 6_000L);
        if ("not_found".equals(result)) {
            return error("未找到目标元素，请重新 observe_refs");
        }
        if (result == null) {
            return error("点击超时");
        }
        sleep(650);
        return pageText("已点击目标元素");
    }

    private static String type(JSONObject args) {
        if (!ensureReady()) {
            return error("浏览器尚未打开");
        }
        String lookup = lookupExpression(args);
        if (lookup == null) {
            return error("type 需要 ref 或 selector");
        }
        String text = args.optString("text", "");
        boolean submit = args.optBoolean("submit", false);
        String script = "(function(){"
                + "var el=" + lookup + ";"
                + "if(!el)return 'not_found';"
                + "try{el.focus();}catch(e){}"
                + "if(el.isContentEditable){el.textContent=" + JSONObject.quote(text) + ";}"
                + "else if(el.tagName==='INPUT'||el.tagName==='TEXTAREA'){"
                + "var proto=el.tagName==='TEXTAREA'?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;"
                + "var desc=Object.getOwnPropertyDescriptor(proto,'value');"
                + "if(desc&&desc.set){desc.set.call(el," + JSONObject.quote(text) + ");}else{el.value=" + JSONObject.quote(text) + ";}"
                + "}else{return 'not_editable';}"
                + "try{el.dispatchEvent(new Event('input',{bubbles:true}));el.dispatchEvent(new Event('change',{bubbles:true}));}catch(e){}"
                + (submit
                    ? "try{var f=el.form;if(f&&f.requestSubmit){f.requestSubmit();}else{"
                        + "['keydown','keypress','keyup'].forEach(function(t){el.dispatchEvent(new KeyboardEvent(t,{key:'Enter',code:'Enter',keyCode:13,which:13,bubbles:true}));});}}catch(e){}"
                    : "")
                + "return 'typed';})()";
        String result = evalJs(script, 6_000L);
        if ("not_found".equals(result)) {
            return error("未找到输入元素，请重新 observe_refs");
        }
        if ("not_editable".equals(result)) {
            return error("目标元素不可输入");
        }
        if (result == null) {
            return error("输入超时");
        }
        sleep(250);
        return pageText("已输入文本");
    }

    private static String press(JSONObject args) {
        if (!ensureReady()) {
            return error("浏览器尚未打开");
        }
        String key = args.optString("key", "").trim();
        int code = keyCode(key);
        if (code == 0) {
            return error("不支持的按键：" + key);
        }
        Boolean sent = onMain(() -> {
            if (webView == null) {
                return Boolean.FALSE;
            }
            long now = android.os.SystemClock.uptimeMillis();
            webView.dispatchKeyEvent(new KeyEvent(now, now, KeyEvent.ACTION_DOWN, code, 0));
            webView.dispatchKeyEvent(new KeyEvent(now, now, KeyEvent.ACTION_UP, code, 0));
            return Boolean.TRUE;
        }, 4_000L);
        if (!Boolean.TRUE.equals(sent)) {
            return error("按键发送失败");
        }
        sleep(250);
        return pageText("已按键 " + key);
    }

    private static String hover(JSONObject args) {
        if (!ensureReady()) {
            return error("浏览器尚未打开");
        }
        String lookup = lookupExpression(args);
        if (lookup == null) {
            return error("hover 需要 ref 或 selector");
        }
        String script = "(function(){var el=" + lookup + ";if(!el)return 'not_found';"
                + "var r=el.getBoundingClientRect();"
                + "['pointerover','mouseover','mousemove'].forEach(function(t){try{el.dispatchEvent(new MouseEvent(t,{bubbles:true,cancelable:true,clientX:r.left+r.width/2,clientY:r.top+r.height/2}));}catch(e){}});"
                + "return 'hovered';})()";
        String result = evalJs(script, 5_000L);
        if ("not_found".equals(result)) {
            return error("未找到目标元素");
        }
        return result == null ? error("悬停超时") : pageText("已悬停目标元素");
    }

    private static String select(JSONObject args) {
        if (!ensureReady()) {
            return error("浏览器尚未打开");
        }
        String lookup = lookupExpression(args);
        if (lookup == null) {
            return error("select 需要 ref 或 selector");
        }
        Object value = args.opt("value");
        String script = "(function(){var el=" + lookup + ";if(!el)return 'not_found';"
                + "var values=" + jsonValueExpression(value) + ";"
                + "if(!Array.isArray(values))values=[values];"
                + "try{for(var i=0;i<el.options.length;i++){el.options[i].selected=values.indexOf(el.options[i].value)>=0;}"
                + "el.dispatchEvent(new Event('input',{bubbles:true}));el.dispatchEvent(new Event('change',{bubbles:true}));return 'selected';}catch(e){return 'not_select';})()";
        String result = evalJs(script, 5_000L);
        if ("not_found".equals(result)) {
            return error("未找到下拉元素");
        }
        if ("not_select".equals(result)) {
            return error("目标元素不是下拉框");
        }
        return result == null ? error("选择超时") : pageText("已选择选项");
    }

    private static String drag(JSONObject args) {
        if (!ensureReady()) {
            return error("浏览器尚未打开");
        }
        String from = lookupExpression(args, "from_ref", "from_selector");
        String to = lookupExpression(args, "to_ref", "to_selector");
        if (from == null || to == null) {
            return error("drag 需要 from_ref/from_selector 与 to_ref/to_selector");
        }
        String script = "(function(){var a=" + from + ",b=" + to + ";if(!a||!b)return 'not_found';"
                + "var r1=a.getBoundingClientRect(),r2=b.getBoundingClientRect();"
                + "var x1=r1.left+r1.width/2,y1=r1.top+r1.height/2,x2=r2.left+r2.width/2,y2=r2.top+r2.height/2;"
                + "['pointerdown','mousedown'].forEach(function(t){try{a.dispatchEvent(new MouseEvent(t,{bubbles:true,cancelable:true,clientX:x1,clientY:y1}));}catch(e){}});"
                + "['pointermove','mousemove'].forEach(function(t){try{b.dispatchEvent(new MouseEvent(t,{bubbles:true,cancelable:true,clientX:x2,clientY:y2}));}catch(e){}});"
                + "['pointerup','mouseup'].forEach(function(t){try{b.dispatchEvent(new MouseEvent(t,{bubbles:true,cancelable:true,clientX:x2,clientY:y2}));}catch(e){}});"
                + "return 'dragged';})()";
        String result = evalJs(script, 6_000L);
        if ("not_found".equals(result)) {
            return error("未找到拖拽元素");
        }
        return result == null ? error("拖拽超时") : pageText("已拖拽目标元素");
    }

    private static String waitFor(JSONObject args) {
        int waitMs = clamp(args.optInt("wait_ms", 0), 0, 10_000);
        String selector = args.optString("selector", "").trim();
        if (selector.isEmpty()) {
            sleep(waitMs);
            return pageText("已等待 " + waitMs + " ms");
        }
        long timeout = clamp(args.optInt("timeout_ms", 10_000), 1_000, 30_000);
        String state = args.optString("state", "").trim().toLowerCase(Locale.ROOT);
        String quoted = JSONObject.quote(selector);
        String script;
        switch (state) {
            case "hidden":
                script = "(function(){var el=document.querySelector(" + quoted + ");if(!el)return true;"
                        + "var r=el.getBoundingClientRect();var s=getComputedStyle(el);"
                        + "return !(r.width>0&&r.height>0&&s.visibility!=='hidden'&&s.display!=='none'&&parseFloat(s.opacity||'1')>0.05);})()";
                break;
            case "detached":
                script = "(function(){return document.querySelector(" + quoted + ")==null;})()";
                break;
            default:
                script = "(function(){return document.querySelector(" + quoted + ")!=null;})()";
                break;
        }
        long deadline = System.currentTimeMillis() + timeout;
        while (System.currentTimeMillis() < deadline) {
            String result = evalJs(script, 3_000L);
            if ("true".equals(result)) {
                return pageText(state.isEmpty() || "visible".equals(state) || "attached".equals(state)
                        ? "元素已出现：" + selector
                        : "页面状态已满足：" + selector + " " + state);
            }
            sleep(250);
        }
        return error("等待页面状态超时：" + selector);
    }

    private static String screenshot() {
        if (!ensureReady()) {
            return error("浏览器尚未打开");
        }
        String base64 = onMain(() -> {
            if (webView == null || webView.getWidth() <= 0 || webView.getHeight() <= 0) {
                return null;
            }
            Bitmap bitmap = Bitmap.createBitmap(
                    webView.getWidth(), webView.getHeight(), Bitmap.Config.ARGB_8888);
            Canvas canvas = new Canvas(bitmap);
            webView.draw(canvas);
            int maxWidth = 900;
            if (bitmap.getWidth() > maxWidth) {
                int height = Math.max(1, bitmap.getHeight() * maxWidth / bitmap.getWidth());
                Bitmap scaled = Bitmap.createScaledBitmap(bitmap, maxWidth, height, true);
                bitmap.recycle();
                bitmap = scaled;
            }
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, out);
            bitmap.recycle();
            return Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP);
        }, 12_000L);
        if (base64 == null || base64.isEmpty()) {
            return error("截图失败");
        }
        try {
            JSONObject result = new JSONObject();
            result.put("ok", true);
            result.put("text", "已截取当前页面");
            result.put("image_base64", base64);
            result.put("mime_type", "image/png");
            return result.toString();
        } catch (Throwable error) {
            return error(message(error));
        }
    }

    private static String tabsList() {
        String url = currentUrl;
        String title = currentTitle;
        String text = "1 tab\n- tab-1 " + (title.isEmpty() ? url : title)
                + (url.isEmpty() ? "" : " (" + url + ")");
        return pageText(text);
    }

    private static String status() {
        return pageText((currentTitle.isEmpty() ? "浏览器" : currentTitle)
                + (currentUrl.isEmpty() ? "" : "\n" + currentUrl));
    }

    private static String close() {
        onMain(() -> {
            destroyBrowserOnMain();
            return null;
        }, 6_000L);
        return ok("浏览器已关闭");
    }

    // ---- browser lifecycle -------------------------------------------------

    private static void ensureBrowser() {
        if (webView != null) {
            return;
        }
        if (!Settings.canDrawOverlays(app)) {
            requestOverlayPermission();
            throw new IllegalStateException("需要悬浮窗权限，请在系统设置中允许安卓版显示在其他应用上层后重试");
        }
        onMain(() -> {
            createBrowserOnMain();
            return null;
        }, 10_000L);
        if (webView == null) {
            throw new IllegalStateException("无法创建浏览器窗口");
        }
    }

    private static boolean ensureReady() {
        try {
            ensureBrowser();
            return true;
        } catch (RuntimeException error) {
            return false;
        }
    }

    private static void createBrowserOnMain() {
        if (webView != null) {
            return;
        }
        windowManager = (WindowManager) app.getSystemService(Context.WINDOW_SERVICE);
        if (windowManager == null) {
            throw new IllegalStateException("窗口服务不可用");
        }

        LinearLayout root = new LinearLayout(app);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(0xFFF7F7F5);

        LinearLayout header = new LinearLayout(app);
        header.setGravity(Gravity.CENTER_VERTICAL);
        header.setPadding(dp(14), dp(8), dp(6), dp(8));
        header.setBackgroundColor(0xFFFFFFFF);

        titleView = new TextView(app);
        titleView.setText("浏览器");
        titleView.setTextSize(14);
        titleView.setTextColor(0xFF2B2B2B);
        titleView.setSingleLine(true);
        titleView.setEllipsize(android.text.TextUtils.TruncateAt.END);
        header.addView(titleView, new LinearLayout.LayoutParams(
                0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f));

        TextView close = new TextView(app);
        close.setText("×");
        close.setTextSize(24);
        close.setTextColor(0xFF6B6B6B);
        close.setGravity(Gravity.CENTER);
        close.setPadding(dp(10), 0, dp(10), 0);
        close.setOnClickListener(v -> destroyBrowserOnMain());
        header.addView(close, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, dp(40)));

        root.addView(header, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, dp(46)));

        webView = new WebView(app);
        configureWebView(webView);
        root.addView(webView, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f));

        int height = Math.max(dp(280), (int) (screenHeight() * 0.62f));
        WindowManager.LayoutParams params = new WindowManager.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                height,
                WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
                WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL
                        | WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
                PixelFormat.TRANSLUCENT);
        params.gravity = Gravity.TOP | Gravity.START;
        params.x = 0;
        params.y = dp(64);
        windowManager.addView(root, params);
        overlay = root;
        updateTitleOnMain();
        Log.i(TAG, "webview overlay created");
    }

    private static void configureWebView(WebView view) {
        WebSettings settings = view.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setDatabaseEnabled(true);
        settings.setLoadWithOverviewMode(true);
        settings.setUseWideViewPort(true);
        settings.setSupportZoom(true);
        settings.setBuiltInZoomControls(true);
        settings.setDisplayZoomControls(false);
        settings.setMixedContentMode(WebSettings.MIXED_CONTENT_COMPATIBILITY_MODE);
        settings.setCacheMode(WebSettings.LOAD_DEFAULT);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            settings.setSafeBrowsingEnabled(true);
        }
        CookieManager.getInstance().setAcceptCookie(true);
        view.setBackgroundColor(Color.WHITE);
        view.setWebChromeClient(new WebChromeClient() {
            @Override
            public void onReceivedTitle(WebView web, String title) {
                if (title != null) {
                    currentTitle = title;
                    updateTitleOnMain();
                }
            }
        });
        view.setWebViewClient(new WebViewClient() {
            @Override
            public void onPageStarted(WebView web, String url, Bitmap favicon) {
                currentUrl = url == null ? "" : url;
                lastLoadError = "";
            }

            @Override
            public void onPageFinished(WebView web, String url) {
                if (url != null) {
                    currentUrl = url;
                }
                String title = web.getTitle();
                if (title != null && !title.isEmpty()) {
                    currentTitle = title;
                }
                updateTitleOnMain();
                CountDownLatch latch = pageLatch;
                if (latch != null) {
                    latch.countDown();
                }
            }

            @Override
            public void onReceivedError(WebView web, WebResourceRequest request,
                                        android.webkit.WebResourceError error) {
                if (request != null && request.isForMainFrame()) {
                    lastLoadError = error == null ? "页面加载失败" : String.valueOf(error.getDescription());
                }
            }

            @Override
            public boolean shouldOverrideUrlLoading(WebView web, WebResourceRequest request) {
                Uri uri = request == null ? null : request.getUrl();
                return uri != null && !isHttpUrl(uri.toString());
            }

            @Override
            public boolean shouldOverrideUrlLoading(WebView web, String url) {
                return !isHttpUrl(url);
            }
        });
    }

    private static void destroyBrowserOnMain() {
        CountDownLatch latch = pageLatch;
        if (latch != null) {
            latch.countDown();
            pageLatch = null;
        }
        if (webView != null) {
            try {
                webView.stopLoading();
                webView.loadUrl("about:blank");
                webView.clearHistory();
                webView.destroy();
            } catch (Throwable ignored) {
            }
            webView = null;
        }
        if (overlay != null && windowManager != null) {
            try {
                windowManager.removeView(overlay);
            } catch (Throwable ignored) {
            }
        }
        overlay = null;
        titleView = null;
        currentUrl = "";
        currentTitle = "";
        Log.i(TAG, "webview overlay destroyed");
    }

    private static void requestOverlayPermission() {
        long now = System.currentTimeMillis();
        if (now - lastPermissionRequestAt < 8_000L) {
            return;
        }
        lastPermissionRequestAt = now;
        try {
            Intent intent = new Intent(
                    Settings.ACTION_MANAGE_OVERLAY_PERMISSION,
                    Uri.parse("package:" + app.getPackageName()));
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            app.startActivity(intent);
        } catch (Throwable error) {
            Log.w(TAG, "open overlay settings failed", error);
        }
    }

    // ---- JS and window helpers --------------------------------------------

    private static String evalJs(String script, long timeoutMs) {
        if (webView == null) {
            return null;
        }
        CountDownLatch latch = new CountDownLatch(1);
        AtomicReference<String> result = new AtomicReference<>();
        onMain(() -> {
            if (webView != null) {
                webView.evaluateJavascript(script, value -> {
                    result.set(value);
                    latch.countDown();
                });
            } else {
                latch.countDown();
            }
            return null;
        }, 5_000L);
        try {
            if (!latch.await(timeoutMs, TimeUnit.MILLISECONDS)) {
                return null;
            }
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
            return null;
        }
        return decodeJsString(result.get());
    }

    private static String lookupExpression(JSONObject args) {
        String ref = args.optString("ref", "").trim();
        String selector = args.optString("selector", "").trim();
        if (!ref.isEmpty()) {
            if (!ref.matches("[A-Za-z0-9_-]+")) {
                return null;
            }
            return "document.querySelector('[data-wunder-ref=\"" + ref + "\"]')";
        }
        if (!selector.isEmpty()) {
            return "document.querySelector(" + JSONObject.quote(selector) + ")";
        }
        return null;
    }

    private static String lookupExpression(JSONObject args, String refKey, String selectorKey) {
        JSONObject copy = new JSONObject();
        try {
            copy.put("ref", args.optString(refKey, ""));
            copy.put("selector", args.optString(selectorKey, ""));
        } catch (Throwable ignored) {
        }
        return lookupExpression(copy);
    }

    private static String jsonValueExpression(Object value) {
        if (value == null || value == JSONObject.NULL) {
            return "null";
        }
        if (value instanceof org.json.JSONArray
                || value instanceof JSONObject
                || value instanceof Number
                || value instanceof Boolean) {
            return String.valueOf(value);
        }
        return JSONObject.quote(String.valueOf(value));
    }

    private static void updateTitleOnMain() {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            applyTitle();
        } else {
            MAIN.post(WebViewBridge::applyTitle);
        }
    }

    private static void applyTitle() {
        if (titleView == null) {
            return;
        }
        String title = currentTitle == null ? "" : currentTitle.trim();
        if (title.isEmpty()) {
            title = currentUrl == null ? "" : currentUrl.trim();
        }
        titleView.setText(title.isEmpty() ? "浏览器" : title);
    }

    private static <T> T onMain(Callable<T> task, long timeoutMs) {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            try {
                return task.call();
            } catch (Exception error) {
                Log.w(TAG, "main-thread task failed", error);
                return null;
            }
        }
        FutureTask<T> future = new FutureTask<>(task);
        MAIN.post(future);
        try {
            return future.get(timeoutMs, TimeUnit.MILLISECONDS);
        } catch (Exception error) {
            future.cancel(true);
            return null;
        }
    }

    private static void await(CountDownLatch latch, long timeoutMs) {
        try {
            latch.await(timeoutMs, TimeUnit.MILLISECONDS);
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
        }
    }

    private static boolean isHttpUrl(String raw) {
        if (raw == null || raw.isEmpty()) {
            return false;
        }
        try {
            Uri uri = Uri.parse(raw);
            String scheme = uri.getScheme();
            return ("http".equalsIgnoreCase(scheme) || "https".equalsIgnoreCase(scheme))
                    && uri.getHost() != null
                    && !uri.getHost().isEmpty();
        } catch (Throwable error) {
            return false;
        }
    }

    private static int keyCode(String key) {
        switch (key) {
            case "Enter":
                return KeyEvent.KEYCODE_ENTER;
            case "Tab":
                return KeyEvent.KEYCODE_TAB;
            case "Escape":
                return KeyEvent.KEYCODE_ESCAPE;
            case "Backspace":
                return KeyEvent.KEYCODE_DEL;
            case "ArrowUp":
                return KeyEvent.KEYCODE_DPAD_UP;
            case "ArrowDown":
                return KeyEvent.KEYCODE_DPAD_DOWN;
            case "ArrowLeft":
                return KeyEvent.KEYCODE_DPAD_LEFT;
            case "ArrowRight":
                return KeyEvent.KEYCODE_DPAD_RIGHT;
            case "PageUp":
                return KeyEvent.KEYCODE_PAGE_UP;
            case "PageDown":
                return KeyEvent.KEYCODE_PAGE_DOWN;
            case "Space":
                return KeyEvent.KEYCODE_SPACE;
            default:
                return 0;
        }
    }

    private static String decodeJsString(String raw) {
        if (raw == null || "null".equals(raw)) {
            return "";
        }
        try {
            Object value = new JSONTokener(raw).nextValue();
            return value instanceof String ? (String) value : String.valueOf(value);
        } catch (Throwable error) {
            return raw;
        }
    }

    private static String pageText(String text) {
        try {
            JSONObject result = new JSONObject();
            result.put("ok", true);
            if (text != null && !text.isEmpty()) {
                result.put("text", text);
            }
            if (!currentUrl.isEmpty()) {
                result.put("url", currentUrl);
            }
            if (!currentTitle.isEmpty()) {
                result.put("title", currentTitle);
            }
            if (!lastLoadError.isEmpty()) {
                result.put("note", lastLoadError);
            }
            return result.toString();
        } catch (Throwable error) {
            return error(message(error));
        }
    }

    private static String ok(String text) {
        return pageText(text);
    }

    private static String error(String message) {
        try {
            JSONObject result = new JSONObject();
            result.put("ok", false);
            result.put("error", message == null ? "浏览器操作失败" : message);
            if (!currentUrl.isEmpty()) {
                result.put("url", currentUrl);
            }
            return result.toString();
        } catch (Throwable ignored) {
            return "{\"ok\":false,\"error\":\"浏览器操作失败\"}";
        }
    }

    private static String message(Throwable error) {
        String text = error == null ? "" : error.getMessage();
        return text == null || text.trim().isEmpty()
                ? (error == null ? "浏览器操作失败" : error.getClass().getSimpleName())
                : text;
    }

    private static String truncate(String text, int limit) {
        if (text == null) {
            return "";
        }
        if (text.length() <= limit) {
            return text;
        }
        return text.substring(0, limit) + "\n…（已截断）";
    }

    private static int clamp(int value, int min, int max) {
        return Math.max(min, Math.min(max, value));
    }

    private static void sleep(long millis) {
        if (millis <= 0) {
            return;
        }
        try {
            Thread.sleep(millis);
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
        }
    }

    private static int dp(int value) {
        return Math.round(value * app.getResources().getDisplayMetrics().density);
    }

    private static int screenHeight() {
        DisplayMetrics metrics = new DisplayMetrics();
        WindowManager manager = (WindowManager) app.getSystemService(Context.WINDOW_SERVICE);
        if (manager != null) {
            manager.getDefaultDisplay().getRealMetrics(metrics);
            if (metrics.heightPixels > 0) {
                return metrics.heightPixels;
            }
        }
        return app.getResources().getDisplayMetrics().heightPixels;
    }

    /**
     * Builds an accessibility-like tree and assigns stable `data-wunder-ref`
     * attributes so the model can target elements without CSS knowledge.
     */
    private static final String SNAPSHOT_JS =
            "(function(){"
            + "var old=document.querySelectorAll('[data-wunder-ref]');"
            + "for(var i=0;i<old.length;i++)old[i].removeAttribute('data-wunder-ref');"
            + "var selector='a,button,input,textarea,select,summary,[role=\"button\"],[role=\"link\"],[role=\"checkbox\"],[role=\"radio\"],[role=\"tab\"],[role=\"menuitem\"],[contenteditable=\"true\"],[onclick]';"
            + "var all=document.querySelectorAll(selector);"
            + "function visible(el){try{var r=el.getBoundingClientRect();var s=getComputedStyle(el);"
            + "return r.width>2&&r.height>2&&s.visibility!=='hidden'&&s.display!=='none'&&parseFloat(s.opacity||'1')>0.05;}catch(e){return false;}}"
            + "function label(el){var t=el.getAttribute('aria-label')||el.getAttribute('placeholder')||el.getAttribute('alt')||el.getAttribute('title')||'';"
            + "if(!t){if(el.tagName==='INPUT'||el.tagName==='TEXTAREA'||el.tagName==='SELECT')t=el.value||'';else t=(el.innerText||'').trim();}"
            + "return (t||'').replace(/\\s+/g,' ').slice(0,120);}"
            + "function role(el){var tag=el.tagName.toLowerCase();var r=(el.getAttribute('role')||'').toLowerCase();"
            + "if(tag==='a')return 'link';"
            + "if(tag==='button'||r==='button')return 'button';"
            + "if(tag==='select')return 'combobox';"
            + "if(tag==='textarea'||el.isContentEditable)return 'textbox';"
            + "if(tag==='input'){var t=(el.type||'text').toLowerCase();"
            + "if(t==='checkbox')return 'checkbox';if(t==='radio')return 'radio';"
            + "if(t==='submit'||t==='button'||t==='reset')return 'button';return 'textbox';}"
            + "return r||'clickable';}"
            + "var lines=[];var n=0;"
            + "for(var i=0;i<all.length&&n<" + MAX_SNAPSHOT_NODES + ";i++){var el=all[i];if(!visible(el))continue;"
            + "n++;var ref='e'+n;el.setAttribute('data-wunder-ref',ref);"
            + "var line='- '+ref+' '+role(el)+' \"'+label(el).replace(/\"/g,'\\\\\"')+'\"';"
            + "if(el.tagName==='A'&&el.href)line+=' href=\"'+(el.href||'').slice(0,200)+'\"';"
            + "if(el.tagName==='INPUT')line+=' type='+(el.type||'text');"
            + "if(el.disabled)line+=' disabled';"
            + "lines.push(line);}"
            + "return JSON.stringify({url:location.href,title:document.title,text:lines.join('\\n')});"
            + "})()";
}
