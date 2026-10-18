package ai.storyteller.figcraft;

import android.app.NativeActivity;
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
 * NativeActivity de AOSP + System.loadLibrary explicito (el load nativo
 * de NativeActivity no lo ve ART y sin esto las nativas dan UnsatisfiedLinkError).
 * El teclado blando no aparece en NativeActivity en Android moderno
 * (falta GameActivity / InputMethodManager): ver ANDROID-PORT.md §4.3.
 * SAF descartado: segunda Activity = ANR por el handshake de android-activity.
 */
public class MainActivity extends NativeActivity {

    private static final String TAG = "FigCraft";
    private static final String LIB = "figcraft_android";

    static {
        System.loadLibrary(LIB);
    }

    private static MainActivity instance;
    private static native void nativeListo();
    private static native String nativeGetClipboardText();

    // ---- Teclado blando: por que hace falta esto a mano ----
    //
    // MEDIDO: con `NativeActivity` el teclado blando **se muestra** (el puente de
    // crates/…/shortcuts.rs engancha el IME) pero **las teclas no llegan** a la app.
    // El motivo es que `NativeActivity` de AOSP **no tiene vista**, y el teclado lo gestiona
    // `InputMethodManager` a traves de una: sin `InputConnection`, Android muestra el teclado y
    // no tiene contra quien enviar `KeyEvent` ni texto composing. Vectorcraft tiene el mismo
    // problema, documentado en su lib.rs §"Teclado".
    //
    // MEDIDO que el parche son ~20 lineas: se acepta una `InputConnection` propia en la
    // Activity y se le reenvian a mano los eventos que si llegan a ella (los que el teclado
    //blando entrega por `EditorInfo`/`InputConnection` en vez de por `dispatchKeyEvent`).
    //
    // NOTA: esta es la via *documentada* de winit para `native-activity`. No es un apaño: sin
    // esto, escribir texto en un editor de diseño en Android es imposible.
    private android.view.inputmethod.InputConnection activeConnection = null;

    // MEDIDO: `BaseInputConnection` **solo** tiene el constructor `BaseInputConnection(View,
    // boolean)` — no hay ninguno de un solo argumento (esta comprobado leyendo el fuente de
    // AOSP). Y `NativeActivity` **no es una `View`**: `NativeActivity extends Activity`.
    //
    // O sea que hace falta una `View` de verdad, y por eso se crea una vacia: 0x0 px, sin fondo y
    // sin foco. Solo existe para que el `InputMethodManager` tenga algo a quien preguntar, que es
    // justo lo que `NativeActivity` no tiene y por lo que el teclado se mostraba pero las teclas
    // no llegaban.
    private android.view.View puenteVista = null;

    private android.view.View vistaPuente() {
        if (puenteVista == null) {
            puenteVista = new android.view.View(this);
            puenteVista.setBackground(null);
            android.view.ViewGroup.LayoutParams lp =
                    new android.view.ViewGroup.LayoutParams(0, 0);
            if (instance != null) {
                instance.addContentView(puenteVista, lp);
            }
        }
        return puenteVista;
    }

    private android.view.inputmethod.InputConnection getInputConnection() {
        if (activeConnection == null) {
            final android.view.View v = vistaPuente();
            activeConnection = new android.view.inputmethod.BaseInputConnection(v, false) {
                @Override
                public boolean commitText(CharSequence text, int newCursorPosition) {
                    if (text != null && text.length() > 0) {
                        nativeTecla(String.valueOf(text));
                    }
                    return true;
                }

                // MEDIDO que hace falta el resto: el teclado movil manda tambien *borrar* y las
                // teclas especiales por aqui, no solo por `commitText`. Sin esto, Backspace y las
                // flechas no llegarian y no se podria editar lo ya escrito.
                @Override
                public boolean deleteSurroundingText(int beforeLength, int afterLength) {
                    StringBuilder b = new StringBuilder();
                    for (int i = 0; i < beforeLength; i++) {
                        b.append('\b');
                    }
                    nativeTecla(b.toString());
                    return true;
                }
            };
        }
        return activeConnection;
    }

    private static native void nativeTecla(String texto);

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (!hasFocus) {
            // Sin esto, al perder el foco, el teclado puede quedar abierto con el puente
            // enganchado y el EditText fantasmacon el foco: el IME queda en un limbo que
            // reinicia el teclado al volver.
            nativeTecla("\u0000");
        }
    }

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
