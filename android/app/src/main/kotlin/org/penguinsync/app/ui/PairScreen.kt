package org.penguinsync.app.ui

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat

/// Pair screen (docs/design.md §4.6, §9's four screens; §5.2's pairing
/// flow). Camera QR scanning is the primary path — Android is the side that
/// scans (§5.2 step 2) — with a manual paste field kept as a fallback for a
/// denied/absent camera and for testing against an emulator with no camera
/// at all.
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PairScreen(
    fingerprint: String,
    onPair: (String) -> Unit,
) {
    val context = LocalContext.current
    var hasCameraPermission by
        remember {
            mutableStateOf(
                ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
                    PackageManager.PERMISSION_GRANTED,
            )
        }
    val permissionLauncher =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            hasCameraPermission = granted
        }

    // Scanning a QR fires onPair immediately, same as tapping the manual
    // Pair button — this flag just stops a second frame from firing it
    // again a few milliseconds later, before the screen navigates away.
    var scanConsumed by remember { mutableStateOf(false) }
    var manualEntryExpanded by remember { mutableStateOf(!hasCameraPermission) }
    var manualUri by remember { mutableStateOf("") }

    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
    ) {
        Text("This device: $fingerprint", style = MaterialTheme.typography.bodyMedium)
        Spacer(Modifier.height(16.dp))

        if (hasCameraPermission) {
            Text(
                "Point the camera at the QR code shown by `penguinsync` on Linux. " +
                    "Leave some space around it — cropping off a corner stops it from scanning.",
                style = MaterialTheme.typography.bodySmall,
            )
            Spacer(Modifier.height(8.dp))
            Box(
                Modifier
                    .fillMaxWidth()
                    .aspectRatio(3f / 4f)
                    .border(2.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(12.dp)),
            ) {
                QrScannerView(
                    onDecoded = { uri ->
                        if (!scanConsumed && uri.startsWith("penguinsync://")) {
                            scanConsumed = true
                            onPair(uri)
                        }
                    },
                    modifier = Modifier.fillMaxSize(),
                )
            }
        } else {
            Column(
                Modifier
                    .fillMaxWidth()
                    .background(MaterialTheme.colorScheme.surfaceVariant, RoundedCornerShape(12.dp))
                    .padding(16.dp),
            ) {
                Text(
                    "Camera access is needed to scan the pairing QR code. " +
                        "You can still pair by pasting the code below.",
                    style = MaterialTheme.typography.bodyMedium,
                )
                Spacer(Modifier.height(8.dp))
                Button(onClick = { permissionLauncher.launch(Manifest.permission.CAMERA) }) {
                    Text("Grant camera access")
                }
            }
        }

        Spacer(Modifier.height(16.dp))
        TextButton(onClick = { manualEntryExpanded = !manualEntryExpanded }) {
            Text(if (manualEntryExpanded) "Hide manual entry" else "Enter pairing code manually instead")
        }
        if (manualEntryExpanded) {
            OutlinedTextField(
                value = manualUri,
                onValueChange = { manualUri = it },
                label = { Text("penguinsync://pair?...") },
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(8.dp))
            Button(
                onClick = { onPair(manualUri) },
                enabled = manualUri.startsWith("penguinsync://"),
            ) { Text("Pair") }
        }
    }
}
