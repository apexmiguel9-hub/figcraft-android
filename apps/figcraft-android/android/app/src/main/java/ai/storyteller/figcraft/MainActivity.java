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
import android.text.InputType;
import android.util.Log;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

/**
 * La Activity del port (copiada de vectorcraft-android).
 *
 * <p>MEDIDO, y este es el punto en el que el port se dio la vuelta: esto es
 * {@link NativeActivity} de AOSP, <b>no</b> {@code GameActivity} de androidx.games.
 *
 * <p>Se migrated a {@code GameActivity} porque seitchenso que {@code NativeActivity} no es una
 * {@code View} y por tanto no tiene {@code InputConnection}, de modo que el teclado se mostraba
 * pero sus teclas no llegaban. <b>Eso es falso en este telefono</b>: es exactamente lo que hace
 * vectorcraft-android, y ahi el texto si se escribe. Los datos:
 *
 * <ul>
 *   <li>El APK instalado de vectorcraft, extraido del movil, tiene en {@code classes3.dex}:
 *       {@code Landroid/app/NativeActivity;}. No hay ninguna clase de androidx.games.</li>
 *   <li>Los manifests de los dos ports son identicos en {@code windowSoftInputMode="adjustResize"}
 *       y en los mismos {@code configChanges}. El manifest no era la causa.</li>
 *   <li>Con {@code GameActivity}, Android manda el texto al {@code GameTextInput} de
 *       androidx.games y <b>se come las teclas</b>: el logcat solo muestra
 *       {@code D/gti.InputConnection: commitText: h} y ningun evento de teclado.</li>
 *   <li>winit 0.30.13 en Android no traduce eso: {@code set_ime_purpose} esta vacio
 *       ({@code src/platform_impl/android/mod.rs:926}) y {@code WindowEvent::Text} no existe en
 *       todo el crate.</li>
 *   <li>eframe 0.36.2 solo genera {@code Event::Text} en el backend web; egui 0.36.2 solo lo
 *       consume.</li>
 * </ul>
 *
 * <p>O sea: con {@code GameActivity} el texto se quedaba en un sitio del que nadie lo lee, y habia
 * que escribir un puente a mano. Con {@code NativeActivity} Android manda <b>teclas</b> al
 * {@code InputConnection} de AOSP, winit las recibe y el texto sale por ahi.
 *
 * <p>{@code System.loadLibrary} explicito sigue siendo necesario: el load nativo de la Activity no
 * lo ve ART y sin esto las nativas dan {@code UnsatisfiedLinkError}.
 *
 * <p>SAF descartado: segunda Activity = ANR por el handshake de {@code android-activity}.
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
    private static native void nativeInjectText(String texto);

    private static InputView inputView;
    private static InputMethodManager imm;

    // MEDIDO: con `NativeActivity` Android manda **teclas** al `InputConnection` de AOSP, que
    // es justo lo que usa winit. Pero `winit` 0.30.13 no traduce esas teclas a
    // `egui::Event::Text`, así que el teclado blando se muestra pero no escribe nada en la
    // tool de texto. Se añade una `InputView` oculta (0x0 px) con su propio
    // `BaseInputConnection` que reenvía cada `commitText` a Rust vía `nativeInjectText`.
    // Rust lo inyecta como `Event::Text` en `raw_input_hook` y `text_keys` lo procesa igual
    // que en escritorio.

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

        // Vista oculta (0x0 px) con foco y su propio InputConnection para recibir el texto
        // del teclado blando y reenviarlo a Rust vía nativeInjectText.
        inputView = new InputView(this);
        addContentView(inputView, new ViewGroup.LayoutParams(0, 0));
        inputView.requestFocus();

        imm = (InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE);
        if (imm != null) {
            imm.showSoftInput(inputView, InputMethodManager.SHOW_IMPLICIT);
        }
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

    /**
     * Vista oculta (0x0 px) que retiene el foco del teclado y expone un
     * {@link BaseInputConnection} propio. Cada {@code commitText} que el teclado blando
     * entregue se reenvía a Rust vía {@link #nativeInjectText}.
     */
    private static class InputView extends View {

        public InputView(Context context) {
            super(context);
            setFocusable(true);
            setFocusableInTouchMode(true);
            setVisibility(View.INVISIBLE);
        }

        @Override
        public InputConnection onCreateInputConnection(EditorInfo outAttrs) {
            outAttrs.inputType = InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS;
            outAttrs.imeOptions = EditorInfo.IME_ACTION_DONE;
            return new BaseInputConnection(this, true) {
                @Override
                public boolean commitText(CharSequence text, int newCursorPosition) {
                    if (text != null && text.length() > 0 && instance != null) {
                        nativeInjectText(text.toString());
                    }
                    return true;
                }

                @Override
                public boolean sendKeyEvent(KeyEvent event) {
                    // Dejamos que las teclas sigan su camino normal hacia winit/egui.
                    return super.sendKeyEvent(event);
                }
            };
        }
    }
}
