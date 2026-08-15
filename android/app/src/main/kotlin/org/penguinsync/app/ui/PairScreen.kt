package org.penguinsync.app.ui

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Keyboard
import androidx.compose.material.icons.outlined.NoPhotography
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
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
fun PairScreen(onPair: (String) -> Unit) {
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
        if (hasCameraPermission) {
            Text(
                "Point the camera at the QR code shown by penguinsync on Linux. " +
                    "Leave some space around it — cropping off a corner stops it from scanning.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(12.dp))
            Box(
                Modifier
                    .fillMaxWidth()
                    .aspectRatio(3f / 4f)
                    .clip(MaterialTheme.shapes.extraLarge),
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
                // Drawn over the preview rather than around it: the border is
                // the aiming aid, so it has to sit on the image the user is
                // actually pointing at something.
                Box(
                    Modifier
                        .fillMaxSize()
                        .border(
                            width = 3.dp,
                            color = MaterialTheme.colorScheme.primary,
                            shape = MaterialTheme.shapes.extraLarge,
                        ),
                )
            }
        } else {
            Card(
                Modifier.fillMaxWidth(),
                shape = MaterialTheme.shapes.extraLarge,
                colors =
                    CardDefaults.cardColors(
                        containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                    ),
            ) {
                Column(Modifier.padding(20.dp)) {
                    Icon(
                        Icons.Outlined.NoPhotography,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.size(32.dp),
                    )
                    Spacer(Modifier.height(12.dp))
                    Text("Camera access needed", style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.height(4.dp))
                    Text(
                        "Scanning the pairing QR code needs the camera. You can still pair by " +
                            "pasting the code below.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(Modifier.height(16.dp))
                    Button(onClick = { permissionLauncher.launch(Manifest.permission.CAMERA) }) {
                        Text("Grant camera access")
                    }
                }
            }
        }

        Spacer(Modifier.height(16.dp))
        TextButton(onClick = { manualEntryExpanded = !manualEntryExpanded }) {
            Icon(Icons.Outlined.Keyboard, contentDescription = null, Modifier.size(18.dp))
            Spacer(Modifier.width(8.dp))
            Text(if (manualEntryExpanded) "Hide manual entry" else "Enter the pairing code instead")
        }
        AnimatedVisibility(visible = manualEntryExpanded) {
            Column(Modifier.fillMaxWidth()) {
                OutlinedTextField(
                    value = manualUri,
                    onValueChange = { manualUri = it },
                    label = { Text("penguinsync://pair?…") },
                    shape = MaterialTheme.shapes.large,
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(12.dp))
                Button(
                    onClick = { onPair(manualUri) },
                    enabled = manualUri.startsWith("penguinsync://"),
                    modifier = Modifier.align(Alignment.End),
                ) { Text("Pair") }
            }
        }
    }
}
