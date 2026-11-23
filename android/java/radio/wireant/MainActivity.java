package radio.wireant;

import android.app.NativeActivity;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.provider.OpenableColumns;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;

/**
 * The Rust app (libwireant.so) with Android's own file picker (Storage Access Framework).
 * NativeActivity can't receive activity results, so this subclass starts the picker,
 * copies files to and from the chosen document, and reports back through nativeResult.
 */
public class MainActivity extends NativeActivity {
    static final int PICK = 1, CREATE = 2;

    static {
        // NativeActivity loads the library too, but only System.loadLibrary makes its JNI
        // functions (nativeResult) visible to Java
        System.loadLibrary("wireant");
    }

    /** kind = PICK or CREATE; uri null = cancelled; error non-null = it failed. */
    static native void nativeResult(int kind, String uri, String path, String error);

    private String pendingSrc;

    /** Called from Rust: let the user choose a file to open. */
    public void pickFile() {
        runOnUiThread(new Runnable() { public void run() {
            Intent i = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            i.addCategory(Intent.CATEGORY_OPENABLE);
            i.setType("*/*");
            startActivityForResult(i, PICK);
        }});
    }

    /** Called from Rust: let the user choose where to save the file at src, named name. */
    public void createFile(final String name, final String src) {
        runOnUiThread(new Runnable() { public void run() {
            pendingSrc = src;
            Intent i = new Intent(Intent.ACTION_CREATE_DOCUMENT);
            i.addCategory(Intent.CATEGORY_OPENABLE);
            i.setType(mimeFor(name));
            i.putExtra(Intent.EXTRA_TITLE, name);
            startActivityForResult(i, CREATE);
        }});
    }

    /** Called from Rust: copy a local file to a document. Returns null or the error. */
    public String writeUri(String uri, String src) {
        try {
            copy(new FileInputStream(src), getContentResolver().openOutputStream(Uri.parse(uri), "wt"));
            return null;
        } catch (Exception e) {
            return e.toString();
        }
    }

    @Override
    protected void onActivityResult(int req, int res, Intent data) {
        super.onActivityResult(req, res, data);
        if (req != PICK && req != CREATE) return;
        if (res != RESULT_OK || data == null || data.getData() == null) {
            nativeResult(req, null, null, null);
            return;
        }
        Uri uri = data.getData();
        try {
            // keep the right to write saves back to it
            getContentResolver().takePersistableUriPermission(uri,
                    Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
        } catch (Exception ignored) {
            // read-only providers: saving back will fail and say so
        }
        try {
            if (req == PICK) {
                // a folder per document, so two files of the same name don't collide
                File dir = new File(getFilesDir(), "docs/" + Integer.toHexString(uri.toString().hashCode()));
                dir.mkdirs();
                File dst = new File(dir, displayName(uri));
                copy(getContentResolver().openInputStream(uri), new FileOutputStream(dst));
                nativeResult(PICK, uri.toString(), dst.getPath(), null);
            } else {
                copy(new FileInputStream(pendingSrc), getContentResolver().openOutputStream(uri, "wt"));
                nativeResult(CREATE, uri.toString(), pendingSrc, null);
            }
        } catch (Exception e) {
            nativeResult(req, uri.toString(), null, e.toString());
        }
    }

    private String displayName(Uri uri) {
        try (Cursor c = getContentResolver().query(uri, new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (c != null && c.moveToFirst()) {
                String n = c.getString(0);
                if (n != null && !n.isEmpty()) return n.replace('/', '_');
            }
        } catch (Exception ignored) {
        }
        return "antenna.maa";
    }

    private static String mimeFor(String name) {
        String n = name.toLowerCase();
        if (n.endsWith(".pdf")) return "application/pdf";
        if (n.endsWith(".csv")) return "text/csv";
        if (n.endsWith(".png")) return "image/png";
        // .maa, .nec, .ez: a generic type, so the provider doesn't add an extension
        return "application/octet-stream";
    }

    private static void copy(InputStream in, OutputStream out) throws Exception {
        try (InputStream i = in; OutputStream o = out) {
            byte[] buf = new byte[65536];
            int n;
            while ((n = i.read(buf)) > 0) o.write(buf, 0, n);
        }
    }
}
