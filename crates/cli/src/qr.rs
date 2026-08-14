//! Render a pairing URI as a scannable QR code in the terminal.

/// Two rows of pixels per printed line (`Dense1x2`, half-block characters),
/// so the code isn't twice as tall as it needs to be in a terminal cell
/// grid.
pub fn render(data: &str) -> Result<String, qrcode::types::QrError> {
    let code = qrcode::QrCode::new(data.as_bytes())?;
    Ok(code
        .render::<qrcode::render::unicode::Dense1x2>()
        .quiet_zone(true)
        .build())
}
