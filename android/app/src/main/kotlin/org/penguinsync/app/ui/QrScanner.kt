package org.penguinsync.app.ui

import android.os.Handler
import android.os.Looper
import android.util.Log
import android.util.Size
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.core.resolutionselector.AspectRatioStrategy
import androidx.camera.core.resolutionselector.ResolutionSelector
import androidx.camera.core.resolutionselector.ResolutionStrategy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.google.zxing.BarcodeFormat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.LuminanceSource
import com.google.zxing.MultiFormatReader
import com.google.zxing.NotFoundException
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.ReaderException
import com.google.zxing.common.HybridBinarizer
import java.util.concurrent.Executors

/// Camera preview that decodes QR codes as they appear, for the Pair
/// screen's primary flow (docs/design.md §4.6, §5.2 — "Android scans"). Pure
/// CameraX + zxing `core`, no ML Kit / Google Play Services dependency, so
/// this doesn't compromise the F-Droid target (docs/design.md §1).
///
/// Calls [onDecoded] at most once — after the first successful decode it
/// stops analyzing frames. The caller (the Pair screen) owns what happens
/// next; this composable's job ends at "here's a string".
@Composable
fun QrScannerView(
    onDecoded: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    // Decoding runs off the main thread: a 1280x960 frame goes through
    // HybridBinarizer plus up to two full decode attempts (see
    // [QrCodeAnalyzer]), which is far too much to put on the thread that also
    // has to draw the preview. One thread, not a pool — STRATEGY_KEEP_ONLY_LATEST
    // means there is never more than one frame worth decoding at a time.
    val executor = remember { Executors.newSingleThreadExecutor() }
    val mainHandler = remember { Handler(Looper.getMainLooper()) }
    val latestOnDecoded by rememberUpdatedState(onDecoded)
    val analyzer =
        remember {
            // Hop back to the main thread before handing the result up: the
            // Pair screen navigates on decode, and that is main-thread-only.
            QrCodeAnalyzer { text -> mainHandler.post { latestOnDecoded(text) } }
        }

    DisposableEffect(context) {
        onDispose {
            executor.shutdown()
            // The lifecycle we bind to is the Activity's, which stays STARTED
            // when the Pair screen is navigated away from — without an
            // explicit unbind the camera keeps streaming (and burning battery)
            // behind the Devices screen.
            val future = ProcessCameraProvider.getInstance(context)
            future.addListener(
                { runCatching { future.get().unbindAll() } },
                ContextCompat.getMainExecutor(context),
            )
        }
    }

    AndroidView(
        modifier = modifier,
        factory = { ctx ->
            val previewView = PreviewView(ctx)
            val cameraProviderFuture = ProcessCameraProvider.getInstance(ctx)
            cameraProviderFuture.addListener({
                val cameraProvider = cameraProviderFuture.get()
                val preview =
                    Preview.Builder().build().also {
                        it.surfaceProvider = previewView.surfaceProvider
                    }
                val analysis =
                    ImageAnalysis.Builder()
                        // Bigger than CameraX's 640x480 analysis default
                        // because of how much data the pairing QR carries: a
                        // ~195-character `penguinsync://pair?...` URI is a
                        // version-12 code, 65 modules across. At 640 wide,
                        // with the code filling maybe half the frame, that is
                        // ~5 pixels per module — right at the edge of what
                        // survives a little motion blur. 1280 doubles the
                        // margin. Same 4:3 aspect ratio as Preview, so the
                        // analysed frame still shows what the preview shows.
                        .setResolutionSelector(
                            ResolutionSelector.Builder()
                                .setAspectRatioStrategy(AspectRatioStrategy.RATIO_4_3_FALLBACK_AUTO_STRATEGY)
                                .setResolutionStrategy(
                                    ResolutionStrategy(
                                        Size(1280, 960),
                                        ResolutionStrategy.FALLBACK_RULE_CLOSEST_HIGHER_THEN_LOWER,
                                    ),
                                )
                                .build(),
                        )
                        .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                        .build()
                        .also { it.setAnalyzer(executor, analyzer) }
                try {
                    cameraProvider.unbindAll()
                    val camera =
                        cameraProvider.bindToLifecycle(
                            lifecycleOwner,
                            CameraSelector.DEFAULT_BACK_CAMERA,
                            preview,
                            analysis,
                        )
                    Log.i(
                        "QrScannerView",
                        "bound: analysis=${analysis.resolutionInfo?.resolution} " +
                            "preview=${preview.resolutionInfo?.resolution} " +
                            "af=${camera.cameraInfo.isFocusMeteringSupported(
                                androidx.camera.core.FocusMeteringAction.Builder(
                                    previewView.meteringPointFactory.createPoint(0.5f, 0.5f),
                                ).build(),
                            )}",
                    )
                } catch (e: Exception) {
                    // Camera already bound elsewhere, or the device genuinely
                    // has none despite the permission being granted — the
                    // Pair screen's manual-entry fallback is still there.
                    Log.w("QrScannerView", "camera bind failed", e)
                }
            }, ContextCompat.getMainExecutor(ctx))
            previewView
        },
    )
}

private class QrCodeAnalyzer(
    private val onDecoded: (String) -> Unit,
) : ImageAnalysis.Analyzer {
    private val reader =
        MultiFormatReader().apply {
            setHints(
                mapOf(
                    DecodeHintType.POSSIBLE_FORMATS to listOf(BarcodeFormat.QR_CODE),
                    // TRY_HARDER trades frame rate for detection on codes that
                    // are small, skewed, or slightly blurred — exactly the
                    // terminal-rendered case. Affordable now that decoding is
                    // off the main thread.
                    DecodeHintType.TRY_HARDER to true,
                ),
            )
        }
    private var decoded = false
    private var frames = 0

    override fun analyze(image: ImageProxy) {
        if (decoded) {
            image.close()
            return
        }
        try {
            frames++
            // Luma plane only — zxing needs grayscale, not full YUV, and
            // ImageAnalysis's default format (YUV_420_888) puts luma first.
            //
            // `rowStride` is the buffer's real row width in bytes and is
            // frequently larger than `image.width` (rows padded to a
            // hardware-friendly alignment) — feeding `image.width` as the
            // buffer's width here instead desyncs every row after the
            // first, scrambling the image into noise zxing can never
            // decode. `PlanarYUVLuminanceSource`'s `dataWidth` must be the
            // stride; `width`/`height` (the crop rect) stay the true image
            // size.
            val plane = image.planes[0]
            // rewind() because analysis now runs on its own thread against
            // buffers CameraX may hand back with a non-zero position.
            val buffer = plane.buffer
            buffer.rewind()
            val bytes = ByteArray(buffer.remaining())
            buffer.get(bytes)
            val source =
                PlanarYUVLuminanceSource(
                    bytes,
                    plane.rowStride,
                    image.height,
                    0,
                    0,
                    image.width,
                    image.height,
                    false,
                )
            if (frames == 1) {
                Log.i(
                    "QrScanner",
                    "first frame: ${image.width}x${image.height} rowStride=${plane.rowStride} " +
                        "pixelStride=${plane.pixelStride} rotation=${image.imageInfo.rotationDegrees} " +
                        "crop=${image.cropRect} bytes=${bytes.size}",
                )
            }
            val result = decode(source)
            if (result == null) {
                // Roughly every 2s of failed frames. Cheap, and it separates
                // "the camera isn't delivering frames" from "frames arrive
                // but nothing decodes" — the two look identical on screen,
                // and telling them apart is most of the work when a scan
                // silently does nothing.
                if (frames % 30 == 0) {
                    Log.i("QrScanner", "$frames frames analyzed, no QR decoded yet")
                }
                return
            }
            Log.i("QrScanner", "decoded after $frames frames: ${result.take(24)}...")
            decoded = true
            onDecoded(result)
        } catch (e: ReaderException) {
            // A QR-shaped pattern was found but didn't check out (motion
            // blur, partial occlusion, glare) — zxing throws several
            // ReaderException subtypes for this (Checksum/Format/...), all
            // as unremarkable as NotFoundException: just try the next
            // frame. Logged at warn rather than swallowed silently, since
            // "finder pattern found but never decodes" is a real failure
            // mode worth telling apart from "no code in frame at all".
            Log.w("QrScanner", "candidate QR pattern didn't decode: $e")
        } finally {
            image.close()
        }
    }

    /// Decode [source], then — if that finds nothing — the same frame with its
    /// luminance inverted. Returns null when neither polarity holds a QR code.
    ///
    /// The inverted attempt is what makes this scanner work against the QR
    /// `penguinsync pair` prints. That renderer draws *dark* modules as block
    /// glyphs, so the code is only the right way round on a light-background
    /// terminal; on a dark one — the overwhelmingly common case — what's on
    /// screen is a photographic negative of a QR code. zxing's
    /// `MultiFormatReader` has no inversion handling of its own and returns
    /// NotFoundException on every single frame, which looks exactly like
    /// "the scanner does nothing". Verified by feeding both polarities of a
    /// real pairing URI through zxing 3.5.4 offline: normal decodes, inverted
    /// is NotFoundException, and vice versa. Phone camera apps and Google Lens
    /// try both, which is why the same QR scans fine in those.
    ///
    /// Handling it here rather than only fixing the terminal renderer also
    /// covers dark-mode QR codes from anywhere else, and costs nothing on the
    /// happy path — the second attempt only runs on frames that already failed.
    private fun decode(source: LuminanceSource): String? {
        for (candidate in listOf(source, source.invert())) {
            try {
                return reader.decodeWithState(BinaryBitmap(HybridBinarizer(candidate))).text
            } catch (_: NotFoundException) {
                // No QR code in this frame at this polarity — the
                // overwhelmingly common case while the user is still lining up
                // the camera. Not an error; fall through to the next polarity.
            }
        }
        return null
    }
}
