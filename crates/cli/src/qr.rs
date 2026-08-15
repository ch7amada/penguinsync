//! Render a pairing URI as a scannable QR code in the terminal.

/// A rendered QR code, together with the terminal cell dimensions it needs.
///
/// Callers must have those dimensions: a QR code that is clipped even one
/// column short is not a degraded QR code, it is not a QR code at all — the
/// cut edge takes a finder pattern with it and every decoder rejects it.
/// Returning the size alongside the text makes "will this fit?" a question
/// the caller is able to ask before drawing.
pub struct Rendered {
    pub text: String,
    /// Width in terminal columns (one column per QR module).
    pub width: u16,
    /// Height in terminal rows. Half the module count, rounded up —
    /// `Dense1x2` packs two module rows into each row of half-block
    /// characters.
    pub height: u16,
}

/// Two rows of pixels per printed line (`Dense1x2`, half-block characters),
/// so the code isn't twice as tall as it needs to be in a terminal cell
/// grid.
///
/// Error correction is deliberately the lowest level. These codes are read
/// off a screen from 20cm away and live for 60 seconds (`TOKEN_TTL`), not
/// printed on a shipping label that has to survive a scuffed corner — and
/// the level drives the module count, which is what decides whether the code
/// fits in the terminal at all. For a ~195-character pairing URI that is the
/// difference between a 57-module code and a 53-module one.
pub fn render(data: &str) -> Result<Rendered, qrcode::types::QrError> {
    let code = qrcode::QrCode::with_error_correction_level(data.as_bytes(), qrcode::EcLevel::L)?;
    let text = code
        .render::<qrcode::render::unicode::Dense1x2>()
        .quiet_zone(true)
        .build();
    let width = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
    let height = text.lines().count();
    Ok(Rendered {
        text,
        width: width.try_into().unwrap_or(u16::MAX),
        height: height.try_into().unwrap_or(u16::MAX),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A realistic pairing URI: `v`, a 32-byte device id, a name, two
    /// candidate addresses, and a 16-byte token (docs/protocol.md §3.1).
    fn sample_uri() -> String {
        format!(
            "penguinsync://pair?v=0&id={}&name=desk-fedora\
             &addr=192.168.1.42%3A58210&addr=127.0.0.1%3A58210&token={}",
            "9f".repeat(32),
            "ab".repeat(16),
        )
    }

    #[test]
    fn reported_size_matches_the_rendered_text() {
        let rendered = render(&sample_uri()).unwrap();
        assert_eq!(rendered.height as usize, rendered.text.lines().count());
        for line in rendered.text.lines() {
            assert!(line.chars().count() <= rendered.width as usize);
        }
        assert!(
            rendered
                .text
                .lines()
                .any(|l| l.chars().count() == rendered.width as usize),
            "at least one line should be exactly the reported width",
        );
    }

    /// The whole point of the reported size: it has to be small enough that
    /// an ordinary terminal can show the code uncut. A code that only fits a
    /// 90-column window is one most people will never manage to scan.
    #[test]
    fn a_realistic_pairing_uri_fits_a_conventional_terminal() {
        let rendered = render(&sample_uri()).unwrap();
        assert!(
            rendered.width <= 80,
            "QR is {} columns wide; won't fit an 80-column terminal",
            rendered.width,
        );
    }
}
