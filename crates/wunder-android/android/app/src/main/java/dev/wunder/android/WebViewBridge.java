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
import android.provider.DocumentsContract;
import android.provider.Settings;
import android.util.Base64;
import android.util.DisplayMetrics;
import android.util.Log;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowManager;
import android.webkit.ConsoleMessage;
import android.webkit.CookieManager;
import android.webkit.JsPromptResult;
import android.webkit.JsResult;
import android.webkit.MimeTypeMap;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.TextView;

import org.json.JSONArray;
import org.json.JSONObject;
import org.json.JSONTokener;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.InputStream;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Deque;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
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
    /** 工作区预览域名：请求由 WebViewClient 从磁盘应答，不做 DNS 解析。 */
    private static final String PREVIEW_HOST = "wunder.preview";
    private static final String PREVIEW_ORIGIN = "https://" + PREVIEW_HOST;

    private static final Handler MAIN = new Handler(Looper.getMainLooper());

    private static Context app;
    private static WindowManager windowManager;
    private static WebView webView;
    private static View overlay;
    private static WindowManager.LayoutParams overlayParams;
    private static TextView titleView;
    private static TextView browserToggle;
    private static int browserHeaderHeight;
    private static int browserExpandedHeight;
    /** 默认展开；用户可手动收起为一条标题栏，收起状态在同一进程内保留。 */
    private static boolean browserMinimized = false;
    private static volatile CountDownLatch pageLatch;
    private static volatile String currentUrl = "";
    private static volatile String currentTitle = "";
    private static volatile String lastLoadError = "";
    private static volatile long lastPermissionRequestAt;
    /** 当前预览的工作区根目录；为空时预览域名返回 503。 */
    private static volatile File previewRoot;

    /** 逻辑视口容器：WebView 按 CSS 视口放大布局，再缩放到可见区域。 */
    private static FrameLayout viewportHost;
    private static int viewportWidthCss;
    private static int viewportHeightCss;

    private static final int MAX_LOG_ENTRIES = 200;
    private static final Object LOG_LOCK = new Object();
    private static final Deque<String> consoleEntries = new ArrayDeque<>();
    private static final Deque<String> networkEntries = new ArrayDeque<>();
    private static volatile ValueCallback<Uri[]> pendingFileChooser;
    private static volatile PendingDialog pendingDialog;

    /** One JS alert/confirm/prompt waiting for the `dialog` action. */
    private static final class PendingDialog {
        final String kind;
        final String message;
        final String defaultValue;
        final JsResult result;
        final JsPromptResult promptResult;

        PendingDialog(String kind, String message, String defaultValue,
                      JsResult result, JsPromptResult promptResult) {
            this.kind = kind;
            this.message = message == null ? "" : message;
            this.defaultValue = defaultValue == null ? "" : defaultValue;
            this.result = result;
            this.promptResult = promptResult;
        }
    }

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
                case "resize":
                    return resize(args);
                case "file_upload":
                    return fileUpload(args);
                case "dialog":
                    return handleDialog(args);
                case "console":
                    return readConsole(args);
                case "network":
                    return readNetwork(args);
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
        String previewPath = args.optString("preview_path", "").trim();
        File previewRootForLoad = null;
        if (!previewPath.isEmpty()) {
            String rootArg = args.optString("preview_root", "").trim();
            try {
                File root = new File(rootArg).getCanonicalFile();
                if (!root.isDirectory()) {
                    return error("工作区预览根目录不可用");
                }
                File target = previewFile(root, previewPath);
                if (target == null) {
                    return error("工作区预览文件不存在：" + previewPath);
                }
                previewRootForLoad = root;
                url = PREVIEW_ORIGIN + "/" + encodePreviewPath(relativePreviewPath(root, target));
            } catch (Throwable failure) {
                return error("工作区预览路径无效：" + previewPath);
            }
        } else {
            // 外部页面不再继承上一次的工作区预览根目录。
            previewRoot = null;
            if (!isHttpUrl(url)) {
                return error("url 必须是 http(s) 地址；预览工作区文件请传 path");
            }
        }
        final String targetUrl = url;
        try {
            ensureBrowser();
        } catch (RuntimeException error) {
            return error(message(error));
        }
        CountDownLatch latch = new CountDownLatch(1);
        pageLatch = latch;
        lastLoadError = "";
        // 浏览器窗口创建后才挂载预览根目录，避免初始化页面把状态清掉。
        previewRoot = previewRootForLoad;
        Boolean started = onMain(() -> {
            if (webView == null) {
                return Boolean.FALSE;
            }
            webView.loadUrl(targetUrl);
            return Boolean.TRUE;
        }, 8_000L);
        if (!Boolean.TRUE.equals(started)) {
            return error("无法启动浏览器窗口");
        }
        await(latch, NAVIGATION_TIMEOUT_MS);
        sleep(350);
        return pageText("已打开 " + (currentUrl.isEmpty() ? targetUrl : currentUrl));
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
            PendingDialog dialog = pendingDialog;
            if (dialog != null) {
                return pageText("已点击，页面弹出 " + dialog.kind + " 对话框，请使用 dialog 处理");
            }
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
            PendingDialog dialog = pendingDialog;
            if (dialog != null) {
                return pageText("输入后页面弹出 " + dialog.kind + " 对话框，请使用 dialog 处理");
            }
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

    private static String resize(JSONObject args) {
        int rawWidth = args.optInt("width", -1);
        int rawHeight = args.optInt("height", -1);
        if (rawWidth < 240 || rawHeight < 240) {
            return error("resize 需要 width/height（240-3840）");
        }
        int width = clamp(rawWidth, 240, 3_840);
        int height = clamp(rawHeight, 240, 3_840);
        float density = app.getResources().getDisplayMetrics().density;
        long pixelWidth = Math.round(width * density);
        long pixelHeight = Math.round(height * density);
        if (pixelWidth > 4096 || pixelHeight > 4096
                || pixelWidth * pixelHeight > 16_000_000L) {
            return error("当前屏幕密度下视口过大，请降低 width/height");
        }
        viewportWidthCss = width;
        viewportHeightCss = height;
        if (webView == null) {
            return pageText("已设置视口 " + width + "x" + height + "，打开页面后生效");
        }
        Boolean applied = onMain(() -> {
            applyViewportLayoutOnMain();
            return Boolean.TRUE;
        }, 5_000L);
        if (!Boolean.TRUE.equals(applied)) {
            return error("设置视口失败");
        }
        sleep(350);
        String actual = evalJs(
                "(function(){return (window.innerWidth||0)+'x'+(window.innerHeight||0);})()",
                3_000L);
        String suffix = actual == null || actual.isEmpty() ? "" : "（实际 " + actual + "）";
        return pageText("视口已设为 " + width + "x" + height + suffix);
    }

    /** Re-layout the WebView at the logical viewport size, then scale it to fit. */
    private static void applyViewportLayoutOnMain() {
        if (webView == null || viewportHost == null) {
            return;
        }
        FrameLayout.LayoutParams params =
                (FrameLayout.LayoutParams) webView.getLayoutParams();
        if (viewportWidthCss <= 0 || viewportHeightCss <= 0) {
            params.width = ViewGroup.LayoutParams.MATCH_PARENT;
            params.height = ViewGroup.LayoutParams.MATCH_PARENT;
        } else {
            float density = app.getResources().getDisplayMetrics().density;
            params.width = Math.round(viewportWidthCss * density);
            params.height = Math.round(viewportHeightCss * density);
        }
        webView.setLayoutParams(params);
        viewportHost.requestLayout();
        viewportHost.post(WebViewBridge::applyViewportTransformOnMain);
    }

    /** Keep the logical viewport visible by uniformly scaling it into the host. */
    private static void applyViewportTransformOnMain() {
        if (webView == null || viewportHost == null) {
            return;
        }
        int hostWidth = viewportHost.getWidth();
        int hostHeight = viewportHost.getHeight();
        if (hostWidth <= 0 || hostHeight <= 0) {
            return;
        }
        float density = app.getResources().getDisplayMetrics().density;
        float logicalWidth = viewportWidthCss > 0
                ? viewportWidthCss * density : hostWidth;
        float logicalHeight = viewportHeightCss > 0
                ? viewportHeightCss * density : hostHeight;
        float scale = Math.min(hostWidth / logicalWidth, hostHeight / logicalHeight);
        if (!Float.isFinite(scale) || scale <= 0f) {
            scale = 1f;
        }
        webView.setPivotX(0f);
        webView.setPivotY(0f);
        webView.setScaleX(scale);
        webView.setScaleY(scale);
        webView.setTranslationX((hostWidth - logicalWidth * scale) / 2f);
        webView.setTranslationY((hostHeight - logicalHeight * scale) / 2f);
    }

    private static String fileUpload(JSONObject args) {
        JSONArray files = args.optJSONArray("files");
        if (files == null || files.length() == 0) {
            return error("file_upload 需要 files");
        }
        if (files.length() > 16) {
            return error("一次最多上传 16 个文件");
        }
        if (webView == null) {
            return error("浏览器尚未打开");
        }
        if (pendingFileChooser == null) {
            String lookup = lookupExpression(args);
            if (lookup != null) {
                evalJs("(function(){var el=" + lookup + ";if(!el)return 'not_found';"
                        + "try{el.click();}catch(e){}return 'clicked';})()", 3_000L);
                sleep(400);
            }
        }
        ValueCallback<Uri[]> callback = pendingFileChooser;
        if (callback == null) {
            return error("当前没有等待中的文件选择器；先点击上传按钮，或提供 ref/selector");
        }
        List<Uri> uris = new ArrayList<>();
        for (int index = 0; index < files.length(); index++) {
            String path = files.optString(index, "").trim();
            if (path.isEmpty()) {
                continue;
            }
            Uri uri = workspaceDocumentUri(new File(path));
            if (uri == null) {
                pendingFileChooser = null;
                onMain(() -> {
                    callback.onReceiveValue(null);
                    return Boolean.TRUE;
                }, 3_000L);
                return error("上传文件必须位于工作区内：" + path);
            }
            uris.add(uri);
        }
        if (uris.isEmpty()) {
            return error("file_upload 需要至少一个文件");
        }
        pendingFileChooser = null;
        Boolean delivered = onMain(() -> {
            callback.onReceiveValue(uris.toArray(new Uri[0]));
            return Boolean.TRUE;
        }, 5_000L);
        if (!Boolean.TRUE.equals(delivered)) {
            return error("提交文件选择失败");
        }
        sleep(300);
        return pageText("已选择 " + uris.size() + " 个文件");
    }

    private static String handleDialog(JSONObject args) {
        PendingDialog dialog = pendingDialog;
        if (dialog == null) {
            return error("当前没有待处理的对话框");
        }
        pendingDialog = null;
        boolean accept = !args.has("accept") || args.optBoolean("accept", true);
        String promptText = args.has("prompt_text")
                ? args.optString("prompt_text", "")
                : dialog.defaultValue;
        Boolean handled = onMain(() -> {
            try {
                if (dialog.promptResult != null) {
                    if (accept) {
                        dialog.promptResult.confirm(promptText);
                    } else {
                        dialog.promptResult.cancel();
                    }
                } else if (dialog.result != null) {
                    if (accept) {
                        dialog.result.confirm();
                    } else {
                        dialog.result.cancel();
                    }
                }
                return Boolean.TRUE;
            } catch (Throwable failure) {
                Log.w(TAG, "dialog handling failed", failure);
                return Boolean.FALSE;
            }
        }, 5_000L);
        if (!Boolean.TRUE.equals(handled)) {
            return error("处理对话框失败");
        }
        return pageText((accept ? "已接受 " : "已取消 ")
                + dialog.kind + " 对话框：" + truncate(dialog.message, 200));
    }

    private static String readConsole(JSONObject args) {
        List<String> entries;
        synchronized (LOG_LOCK) {
            entries = new ArrayList<>(consoleEntries);
            if (args.optBoolean("clear", false)) {
                consoleEntries.clear();
            }
        }
        return pageText(entries.isEmpty()
                ? "(没有控制台消息)" : String.join("\n", entries));
    }

    private static String readNetwork(JSONObject args) {
        List<String> entries;
        synchronized (LOG_LOCK) {
            entries = new ArrayList<>(networkEntries);
            if (args.optBoolean("clear", false)) {
                networkEntries.clear();
            }
        }
        return pageText(entries.isEmpty()
                ? "(没有网络请求记录)" : String.join("\n", entries));
    }

    private static void resetLogs() {
        synchronized (LOG_LOCK) {
            consoleEntries.clear();
            networkEntries.clear();
        }
    }

    private static void recordConsole(String type, String message, String source, int line) {
        JSONObject entry = new JSONObject();
        try {
            entry.put("type", type == null ? "log" : type);
            entry.put("text", truncate(message == null ? "" : message, 2_000));
            if (source != null && !source.isEmpty()) {
                entry.put("source", truncate(source, 500));
            }
            entry.put("line", line);
        } catch (Throwable ignored) {
            return;
        }
        appendLog(consoleEntries, entry.toString());
    }

    private static void recordNetwork(Uri uri, boolean mainFrame, String method) {
        if (uri == null) {
            return;
        }
        String url = uri.toString();
        if (url.startsWith("data:") || url.startsWith("blob:")) {
            return;
        }
        JSONObject entry = new JSONObject();
        try {
            entry.put("url", truncate(url, 2_000));
            entry.put("method", method == null || method.isEmpty() ? "GET" : method);
            entry.put("main_frame", mainFrame);
        } catch (Throwable ignored) {
            return;
        }
        appendLog(networkEntries, entry.toString());
    }

    private static void appendLog(Deque<String> target, String line) {
        synchronized (LOG_LOCK) {
            target.addLast(line);
            while (target.size() > MAX_LOG_ENTRIES) {
                target.removeFirst();
            }
        }
    }

    private static void setPendingDialog(PendingDialog dialog) {
        PendingDialog previous = pendingDialog;
        pendingDialog = dialog;
        if (previous != null) {
            try {
                if (previous.promptResult != null) {
                    previous.promptResult.cancel();
                } else if (previous.result != null) {
                    previous.result.cancel();
                }
            } catch (Throwable ignored) {
                // The old dialog may already be gone.
            }
        }
    }

    private static Uri workspaceDocumentUri(File file) {
        try {
            File target = file.getCanonicalFile();
            for (WorkspaceRegistry.Workspace workspace : WorkspaceRegistry.list()) {
                File root = new File(workspace.root).getCanonicalFile();
                String rootPath = root.getPath();
                String targetPath = target.getPath();
                if (!targetPath.startsWith(rootPath + File.separator)) {
                    continue;
                }
                String relative = targetPath.substring(rootPath.length() + 1)
                        .replace(File.separatorChar, '/');
                String documentId = workspace.id + ":" + relative;
                String authority = app.getPackageName()
                        + WunderDocumentsProvider.AUTHORITY_SUFFIX;
                return DocumentsContract.buildDocumentUri(authority, documentId);
            }
        } catch (Throwable failure) {
            Log.w(TAG, "cannot build workspace document uri", failure);
        }
        return null;
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
        root.setClipChildren(true);
        root.setClipToPadding(true);

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
        titleView.setOnClickListener(v -> setBrowserMinimized(!browserMinimized));
        header.addView(titleView, new LinearLayout.LayoutParams(
                0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f));

        browserToggle = new TextView(app);
        browserToggle.setTextSize(13);
        browserToggle.setTextColor(0xFF2B6CB0);
        browserToggle.setGravity(Gravity.CENTER);
        browserToggle.setPadding(dp(10), 0, dp(10), 0);
        browserToggle.setOnClickListener(v -> setBrowserMinimized(!browserMinimized));
        header.addView(browserToggle, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, dp(40)));

        TextView close = new TextView(app);
        close.setText("×");
        close.setTextSize(24);
        close.setTextColor(0xFF6B6B6B);
        close.setGravity(Gravity.CENTER);
        close.setPadding(dp(10), 0, dp(10), 0);
        close.setOnClickListener(v -> destroyBrowserOnMain());
        header.addView(close, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, dp(40)));

        browserHeaderHeight = dp(46);
        root.addView(header, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, browserHeaderHeight));

        webView = new WebView(app);
        configureWebView(webView);
        // WebView 始终按展开尺寸布局，收起时只裁剪窗口，页面视口与截图尺寸不变。
        int panelHeight = Math.max(dp(160),
                Math.max(dp(280), (int) (screenHeight() * 0.62f)) - browserHeaderHeight);
        viewportHost = new FrameLayout(app);
        viewportHost.setClipChildren(true);
        viewportHost.setClipToPadding(true);
        viewportHost.addOnLayoutChangeListener(
                (v, left, top, right, bottom, oldLeft, oldTop, oldRight, oldBottom) ->
                        applyViewportTransformOnMain());
        root.addView(viewportHost, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, panelHeight));
        viewportHost.addView(webView, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        browserExpandedHeight = browserHeaderHeight + panelHeight;

        overlayParams = new WindowManager.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                browserMinimized ? browserHeaderHeight : browserExpandedHeight,
                WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
                WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL
                        | WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN
                        | (browserMinimized
                                ? WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                                : 0),
                PixelFormat.TRANSLUCENT);
        overlayParams.gravity = Gravity.TOP | Gravity.START;
        overlayParams.x = 0;
        overlayParams.y = dp(64);
        windowManager.addView(root, overlayParams);
        overlay = root;
        updateBrowserToggle();
        updateTitleOnMain();
        applyViewportLayoutOnMain();
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

            @Override
            public boolean onConsoleMessage(ConsoleMessage message) {
                if (message != null) {
                    recordConsole(
                            message.messageLevel() == null
                                    ? "log" : message.messageLevel().name().toLowerCase(Locale.ROOT),
                            message.message(),
                            message.sourceId(),
                            message.lineNumber());
                }
                return true;
            }

            @Override
            public boolean onJsAlert(WebView web, String url, String message, JsResult result) {
                setPendingDialog(new PendingDialog("alert", message, "", result, null));
                return true;
            }

            @Override
            public boolean onJsConfirm(WebView web, String url, String message, JsResult result) {
                setPendingDialog(new PendingDialog("confirm", message, "", result, null));
                return true;
            }

            @Override
            public boolean onJsPrompt(WebView web, String url, String message,
                                      String defaultValue, JsPromptResult result) {
                setPendingDialog(new PendingDialog("prompt", message, defaultValue, null, result));
                return true;
            }

            @Override
            public boolean onShowFileChooser(WebView web, ValueCallback<Uri[]> callback,
                                             FileChooserParams params) {
                pendingFileChooser = callback;
                return true;
            }
        });
        view.setWebViewClient(new WebViewClient() {
            @Override
            public WebResourceResponse shouldInterceptRequest(
                    WebView web, WebResourceRequest request) {
                if (request != null) {
                    recordNetwork(request.getUrl(), request.isForMainFrame(),
                            request.getMethod());
                }
                return interceptPreview(request == null ? null : request.getUrl());
            }

            @Override
            public WebResourceResponse shouldInterceptRequest(WebView web, String url) {
                Uri uri = url == null ? null : Uri.parse(url);
                recordNetwork(uri, false, "GET");
                return interceptPreview(uri);
            }

            @Override
            public void onPageStarted(WebView web, String url, Bitmap favicon) {
                currentUrl = url == null ? "" : url;
                lastLoadError = "";
                resetLogs();
                // 离开预览页就关闭磁盘应答，避免外部页面继续访问工作区文件。
                if (!isPreviewUrl(url)) {
                    previewRoot = null;
                }
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

    // ---- workspace preview ------------------------------------------------

    /**
     * Answer `wunder.preview` from the workspace directory. No HTTP server,
     * no DNS lookup and no cleartext traffic: the WebView asks its client for
     * the bytes before touching the network.
     */
    private static WebResourceResponse interceptPreview(Uri uri) {
        if (uri == null || uri.getHost() == null
                || !PREVIEW_HOST.equalsIgnoreCase(uri.getHost())) {
            return null;
        }
        File root = previewRoot;
        if (root == null) {
            return previewResponse(503, "工作区预览尚未打开");
        }
        String path = uri.getPath();
        String relative = path == null ? "" : path.replaceFirst("^/+", "");
        try {
            File target = previewFile(root, relative);
            if (target == null) {
                return previewResponse(404, "工作区中找不到：" + relative);
            }
            String mime = previewMimeType(target.getName());
            String encoding = isTextMime(mime) ? "utf-8" : null;
            Map<String, String> headers = new HashMap<>();
            headers.put("Cache-Control", "no-store");
            InputStream stream = new FileInputStream(target);
            return new WebResourceResponse(mime, encoding, 200, "OK", headers, stream);
        } catch (Throwable failure) {
            Log.w(TAG, "preview request failed: " + relative, failure);
            return previewResponse(500, "读取工作区文件失败：" + relative);
        }
    }

    /**
     * Resolve a workspace-relative path into a canonical file. Absolute paths,
     * `..` and symlinks escaping the root are rejected; a directory falls back
     * to its index.html.
     */
    private static File previewFile(File root, String relative) throws java.io.IOException {
        String cleaned = relative == null ? "" : relative.replace('\\', '/').trim();
        if (cleaned.startsWith("/") || containsParentSegment(cleaned)) {
            return null;
        }
        File target = new File(root, cleaned).getCanonicalFile();
        if (!isInside(root, target)) {
            return null;
        }
        if (target.isDirectory()) {
            target = new File(target, "index.html").getCanonicalFile();
            if (!isInside(root, target)) {
                return null;
            }
        }
        return target.isFile() ? target : null;
    }

    private static boolean containsParentSegment(String relative) {
        for (String segment : relative.split("/")) {
            if ("..".equals(segment)) {
                return true;
            }
        }
        return false;
    }

    private static boolean isInside(File root, File target) {
        String rootPath = root.getPath();
        String targetPath = target.getPath();
        return targetPath.equals(rootPath)
                || targetPath.startsWith(rootPath + File.separator);
    }

    private static String relativePreviewPath(File root, File target) {
        String rootPath = root.getPath();
        String targetPath = target.getPath();
        if (targetPath.startsWith(rootPath)) {
            String relative = targetPath.substring(rootPath.length());
            while (relative.startsWith(File.separator)) {
                relative = relative.substring(1);
            }
            return relative.replace(File.separatorChar, '/');
        }
        return target.getName();
    }

    /** Encode one workspace-relative path without encoding the separators. */
    private static String encodePreviewPath(String relative) {
        StringBuilder encoded = new StringBuilder();
        for (String segment : relative.replace('\\', '/').split("/")) {
            if (segment.isEmpty()) {
                continue;
            }
            if (encoded.length() > 0) {
                encoded.append('/');
            }
            encoded.append(Uri.encode(segment));
        }
        return encoded.toString();
    }

    private static WebResourceResponse previewResponse(int status, String message) {
        byte[] body = (message + "\n").getBytes(java.nio.charset.StandardCharsets.UTF_8);
        Map<String, String> headers = new HashMap<>();
        headers.put("Cache-Control", "no-store");
        return new WebResourceResponse(
                "text/plain; charset=utf-8",
                "utf-8",
                status,
                status == 200 ? "OK" : "Preview error",
                headers,
                new ByteArrayInputStream(body));
    }

    private static String previewMimeType(String name) {
        int dot = name.lastIndexOf('.');
        String extension = dot >= 0 ? name.substring(dot + 1).toLowerCase(Locale.ROOT) : "";
        switch (extension) {
            case "html":
            case "htm":
                return "text/html";
            case "js":
            case "mjs":
                return "application/javascript";
            case "css":
                return "text/css";
            case "json":
                return "application/json";
            case "wasm":
                return "application/wasm";
            case "svg":
                return "image/svg+xml";
            case "glb":
                return "model/gltf-binary";
            case "gltf":
                return "model/gltf+json";
            case "woff":
                return "font/woff";
            case "woff2":
                return "font/woff2";
            case "ttf":
                return "font/ttf";
            case "otf":
                return "font/otf";
            default:
                String known = MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension);
                return known == null || known.isEmpty() ? "application/octet-stream" : known;
        }
    }

    private static boolean isTextMime(String mime) {
        return mime.startsWith("text/")
                || "application/javascript".equals(mime)
                || "application/json".equals(mime)
                || "image/svg+xml".equals(mime)
                || "model/gltf+json".equals(mime);
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
        overlayParams = null;
        titleView = null;
        browserToggle = null;
        viewportHost = null;
        viewportWidthCss = 0;
        viewportHeightCss = 0;
        pendingFileChooser = null;
        PendingDialog dialog = pendingDialog;
        pendingDialog = null;
        if (dialog != null) {
            try {
                if (dialog.promptResult != null) {
                    dialog.promptResult.cancel();
                } else if (dialog.result != null) {
                    dialog.result.cancel();
                }
            } catch (Throwable ignored) {
            }
        }
        resetLogs();
        browserHeaderHeight = 0;
        browserExpandedHeight = 0;
        currentUrl = "";
        currentTitle = "";
        previewRoot = null;
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

    /**
     * Collapse the overlay to its title bar. The WebView keeps its expanded
     * layout size, so the page viewport, JS coordinates and screenshots stay
     * unchanged while the user gets the screen back.
     */
    private static void setBrowserMinimized(boolean minimized) {
        if (Looper.myLooper() != Looper.getMainLooper()) {
            MAIN.post(() -> setBrowserMinimized(minimized));
            return;
        }
        browserMinimized = minimized;
        if (overlay == null || overlayParams == null || windowManager == null) {
            updateBrowserToggle();
            return;
        }
        overlayParams.height = minimized ? browserHeaderHeight : browserExpandedHeight;
        if (minimized) {
            overlayParams.flags |= WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE;
        } else {
            overlayParams.flags &= ~WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE;
        }
        try {
            windowManager.updateViewLayout(overlay, overlayParams);
        } catch (Throwable error) {
            Log.w(TAG, "resize browser overlay failed", error);
        }
        updateBrowserToggle();
    }

    private static void updateBrowserToggle() {
        if (browserToggle != null) {
            browserToggle.setText(browserMinimized ? "展开" : "收起");
        }
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

    private static boolean isPreviewUrl(String raw) {
        if (raw == null || raw.isEmpty()) {
            return false;
        }
        try {
            return PREVIEW_HOST.equalsIgnoreCase(Uri.parse(raw).getHost());
        } catch (Throwable error) {
            return false;
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
