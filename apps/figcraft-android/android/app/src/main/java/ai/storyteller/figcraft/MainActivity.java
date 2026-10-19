package ai.storyteller.figcraft;

import androidx.games.activity.GameActivity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.os.Environment;
import android.provider.Settings;
import android.util.Log;

/**
 * La Activity del port (copiada de vectorcraft-android).
 *
 * MEDIDO, y es el cambio grande del port: `GameActivity` de androidx.games en vez de
 * `NativeActivity` de AOSP. Con `NativeActivity` el teclado blando se mostraba pero sus teclas
 * no llegaban, porque `NativeActivity` no es una `View` y por tanto no tiene `InputConnection`.
 * En el logcat se veía:
 *
 *     GoogleInputMethodService: onStartInputView() : Dummy InputConnection bound
 *     ImeTracker: onCancelled at PHASE_WM_ABORT_SHOW_IME_POST_LAYOUT
 *
 * `GameActivity` si tiene vista y por tanto `InputConnection`: el teclado funciona sin el puente
 * JNI que hacia falta antes (la clase `BaseInputConnection` y el metodo `nativeTecla` quedan
 * fuera; el texto llega solo por `Event::Text` de egui).
 *
 * `System.loadLibrary` explicito sigue siendo necesario: el load nativo de la Activity no lo ve
 * ART y sin esto las nativas dan `UnsatisfiedLinkError`.
 * SAF descartado: segunda Activity = ANR por el handshake de `android-activity`.
 */
public class MainActivity extends GameActivity {

    private static final String TAG = "FigCraft";
    private static final String LIB = "figcraft_android";

    static {
        System.loadLibrary(LIB);
    }

    private static MainActivity instance;
    private static native void nativeListo();
    private static native String nativeGetClipboardText();

    // MEDIDO: el puente JNI del teclado (`BaseInputConnection`, `vistaPuente`, `nativeTecla`,
    // `onWindowFocusChanged`) **se ha eliminado al migrar a `GameActivity`**. Ese puente existia
    // porque `NativeActivity` no es una `View` y no tiene `InputConnection`, asi que el teclado se
    // mostraba pero sus teclas no llegaban ("Dummy InputConnection bound" en el logcat).
    // `GameActivity` si tiene vista, luego el teclado llega solo por `Event::Text` de egui y el
    // parche sobra.

    private static boolean hayPermiso() {
        try {
            return Environment.isExternalStorageManager();
        } catch (Throwable t) {
            Log.w(TAG, "hayPermiso: " + t);
            return false;
        }
    }

    private static String pedirPermiso() {
        try {
            if (instance == null) {
                return "la Activity todavia no esta lista";
            }
            Intent i = new Intent(
                    Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                    Uri.parse("package:" + instance.getPackageName()));
            i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            instance.startActivity(i);
            return "";
        } catch (Throwable t) {
            Log.w(TAG, "pedirPermiso: " + t);
            return String.valueOf(t);
        }
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        instance = this;
        try {
            nativeListo();
        } catch (Throwable t) {
            Log.w(TAG, "nativeListo: " + t);
        }
        Log.i(TAG, "figcraft: Activity creada, permiso=" + hayPermiso());
    }

    private static String getClipboardText() {
        try {
            ClipboardManager cm = (ClipboardManager) instance.getSystemService(Context.CLIPBOARD_SERVICE);
            if (cm == null) return null;
            ClipData cd = cm.getPrimaryClip();
            if (cd != null && cd.getItemCount() > 0) {
                ClipData.Item item = cd.getItemAt(0);
                CharSequence text = item.getText();
                if (text != null) return text.toString();
            }
        } catch (Throwable t) {
            Log.w(TAG, "getClipboardText: " + t);
        }
        return null;
    }

    private static String insets() {
        try {
            if (instance == null) {
                return "";
            }
            float d = instance.getResources().getDisplayMetrics().density;
            if (d <= 0f) {
                return "";
            }
            android.view.WindowInsets r = instance.getWindow().getDecorView().getRootWindowInsets();
            if (r == null) {
                return "";
            }
            if (android.os.Build.VERSION.SDK_INT >= 30) {
                android.graphics.Insets bars = r.getInsets(
                        android.view.WindowInsets.Type.systemBars() | android.view.WindowInsets.Type.displayCutout());
                return barras(bars.left, bars.top, bars.right, bars.bottom, d);
            }
            @SuppressWarnings("deprecation")
            final int l = r.getSystemWindowInsetLeft();
            @SuppressWarnings("deprecation")
            final int t = r.getSystemWindowInsetTop();
            @SuppressWarnings("deprecation")
            final int ri = r.getSystemWindowInsetRight();
            @SuppressWarnings("deprecation")
            final int b = r.getSystemWindowInsetBottom();
            return barras(l, t, ri, b, d);
        } catch (Throwable t) {
            Log.w(TAG, "insets: " + t);
            return "";
        }
    }

    private static String barras(int left, int top, int right, int bottom, float density) {
        return left / density + "," + top / density + "," + right / density + "," + bottom / density;
    }

    @Override
    protected void onDestroy() {
        instance = null;
        super.onDestroy();
    }

    // Registrado en Rust: Java_ai_storyteller_figcraft_MainActivity_nativeGetClipboardText
    static {
        // Los nativos se resuelven por nombre al cargar la libreria; no hace falta
        // RegisterNatives aqui.
    }
}
