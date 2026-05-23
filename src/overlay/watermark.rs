/// src/overlay/watermark.rs
///
/// Authorization Watermark (Bottom-Right, Subtle)
///
/// **Design**: Professional, non-obtrusive, clearly indicates pre-authorization status
///
/// Styling:
/// - Font: Monospace, 10pt, gray (RGB 128,128,128)
/// - Position: Bottom-right corner, 5px margin
/// - Opacity: 50% (semi-transparent)
/// - Animated pulse (optional): 0.5s fade in/out
/// - Text: "AUTHORIZATION PENDING - EA/DICE REVIEW"

/// Authorization status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationStatus {
    /// Pre-authorization (current state)
    Pending,

    /// Under review by EA/DICE
    UnderReview,

    /// Approved by EA/DICE
    Approved,

    /// Rejected (deployment blocked)
    Rejected,
}

/// Watermark configuration
#[derive(Debug, Clone)]
pub struct AuthorizationWatermark {
    /// Status (determines text and styling)
    status: AuthorizationStatus,

    /// Display text
    text: String,

    /// Font size (points)
    #[allow(dead_code)]
    font_size: f32,

    /// Opacity (0.0 = invisible, 1.0 = opaque)
    opacity: f32,

    /// Pulse animation enabled
    pulse_enabled: bool,

    /// Pulse frequency (Hz)
    pulse_frequency: f32,

    /// Color (RGB, pre-multiplied with opacity)
    color: (f32, f32, f32),
}

impl AuthorizationWatermark {
    /// Create watermark for pending authorization
    pub fn pending() -> Self {
        AuthorizationWatermark {
            status: AuthorizationStatus::Pending,
            text: "⏳ AUTHORIZATION PENDING - EA/DICE REVIEW".to_string(),
            font_size: 10.0,
            opacity: 0.5,
            pulse_enabled: true,
            pulse_frequency: 1.0, // 1 Hz pulse
            color: (128.0, 128.0, 128.0), // Gray
        }
    }

    /// Create watermark for approved authorization
    pub fn approved() -> Self {
        AuthorizationWatermark {
            status: AuthorizationStatus::Approved,
            text: "✅ AUTHORIZED - EA/DICE APPROVED".to_string(),
            font_size: 10.0,
            opacity: 0.3,
            pulse_enabled: false,
            pulse_frequency: 0.0,
            color: (0.0, 200.0, 0.0), // Green
        }
    }

    /// Create watermark for rejected authorization
    pub fn rejected() -> Self {
        AuthorizationWatermark {
            status: AuthorizationStatus::Rejected,
            text: "❌ UNAUTHORIZED - DEPLOYMENT BLOCKED".to_string(),
            font_size: 10.0,
            opacity: 1.0,
            pulse_enabled: true,
            pulse_frequency: 2.0, // 2 Hz pulse (faster for warning)
            color: (255.0, 0.0, 0.0), // Red
        }
    }

    /// Get display text
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Get opacity for frame (with pulse animation)
    pub fn opacity_for_frame(&self, frame_count: u32) -> f32 {
        if !self.pulse_enabled {
            return self.opacity;
        }

        // Pulse: sin(2π * frequency * time)
        // Time = frame_count / 120 Hz
        let time_s = frame_count as f32 / 120.0;
        let pulse_factor = (2.0 * std::f32::consts::PI * self.pulse_frequency * time_s).sin();
        let pulse_normalized = (pulse_factor + 1.0) / 2.0; // Map [-1, 1] to [0, 1]

        // Modulate opacity: base_opacity * (0.3 + 0.7 * pulse_normalized)
        // This keeps it between 30% and 100% of base opacity
        self.opacity * (0.3 + 0.7 * pulse_normalized)
    }

    /// Get RGBA tuple (for ImGui rendering)
    pub fn rgba_for_frame(&self, frame_count: u32) -> (f32, f32, f32, f32) {
        let opacity = self.opacity_for_frame(frame_count);
        // Normalize RGB to [0, 1]
        let r = (self.color.0 / 255.0) * opacity;
        let g = (self.color.1 / 255.0) * opacity;
        let b = (self.color.2 / 255.0) * opacity;
        (r, g, b, opacity)
    }

    /// Get status string
    pub fn status_string(&self) -> &'static str {
        match self.status {
            AuthorizationStatus::Pending => "PENDING",
            AuthorizationStatus::UnderReview => "UNDER_REVIEW",
            AuthorizationStatus::Approved => "APPROVED",
            AuthorizationStatus::Rejected => "REJECTED",
        }
    }
}

impl Default for AuthorizationWatermark {
    fn default() -> Self {
        Self::pending()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watermark_pending() {
        let watermark = AuthorizationWatermark::pending();
        assert_eq!(watermark.status, AuthorizationStatus::Pending);
        assert!(watermark.pulse_enabled);
        assert_eq!(watermark.status_string(), "PENDING");
    }

    #[test]
    fn test_watermark_approved() {
        let watermark = AuthorizationWatermark::approved();
        assert_eq!(watermark.status, AuthorizationStatus::Approved);
        assert!(!watermark.pulse_enabled);
        assert_eq!(watermark.color, (0.0, 200.0, 0.0));
    }

    #[test]
    fn test_watermark_pulse_animation() {
        let watermark = AuthorizationWatermark::pending();

        // At frame 0 (t=0): pulse = sin(0) = 0, normalized = 0.5
        let opacity_0 = watermark.opacity_for_frame(0);
        let expected_0 = 0.5 * (0.3 + 0.7 * 0.5); // 0.5 * 0.65 = 0.325
        assert!((opacity_0 - expected_0).abs() < 0.01);

        // At frame 60 (t=0.5s, freq=1Hz): pulse = sin(π) = 0, normalized = 0.5
        let opacity_60 = watermark.opacity_for_frame(60);
        assert!((opacity_60 - expected_0).abs() < 0.01);

        // At frame 30 (t=0.25s, freq=1Hz): pulse = sin(π/2) = 1, normalized = 1.0
        let opacity_30 = watermark.opacity_for_frame(30);
        let expected_30 = 0.5 * (0.3 + 0.7 * 1.0); // 0.5 * 1.0 = 0.5
        assert!((opacity_30 - expected_30).abs() < 0.01);
    }

    #[test]
    fn test_watermark_rgba_conversion() {
        let watermark = AuthorizationWatermark::pending(); // Gray (128, 128, 128)
        let (r, g, b, a) = watermark.rgba_for_frame(0);

        // Verify RGB is normalized and multiplied by opacity
        let expected_rgb = (128.0 / 255.0) * 0.325; // ~0.161
        assert!((r - expected_rgb).abs() < 0.01);
        assert!((g - expected_rgb).abs() < 0.01);
        assert!((b - expected_rgb).abs() < 0.01);
        assert!((a - 0.325).abs() < 0.01);
    }
}
