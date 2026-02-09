//! Decorative animations for the GUI.
//!
//! Animation functions are pure renderers - they don't schedule repaints.
//! Call `schedule_frame()` once per frame when any animation is visible.

use egui::{Color32, Rect, Rounding, Sense, Stroke, Ui};

/// Minimum glow alpha for the completed session pulse (always clearly visible).
const COMPLETED_GLOW_ALPHA_MIN: f32 = 0.45;
/// Maximum glow alpha for the completed session pulse (full intensity at peak).
const COMPLETED_GLOW_ALPHA_MAX: f32 = 1.0;
/// Period of the completed glow pulse cycle in seconds.
const COMPLETED_GLOW_PERIOD: f64 = 2.0;
// =============================================================================
// Animation Constants
// =============================================================================

/// Animation frame interval (~30fps for smooth animation with low CPU).
const FRAME_INTERVAL_MS: u64 = 33;

// -----------------------------------------------------------------------------
// Rising Particles Animation (US-005)
// -----------------------------------------------------------------------------

/// Horizontal drift amplitude for particle wobble effect.
/// Controls how far particles sway side-to-side as they rise, creating organic movement.
/// Higher values = more pronounced horizontal oscillation.
const PARTICLE_DRIFT: f32 = 6.0;

/// Alpha multiplier applied to all particles.
/// Keeps particles subtle and non-distracting against the background.
/// Range 0.0-1.0, where 1.0 would be fully opaque.
const PARTICLE_ALPHA_MULTIPLIER: f32 = 0.7;

// -----------------------------------------------------------------------------
// Progress Bar Shimmer Animation (US-005)
// -----------------------------------------------------------------------------

/// Maximum width in pixels of the shimmer highlight effect.
/// The shimmer is capped at this width or 30% of the filled bar, whichever is smaller.
/// Controls the visual "size" of the sweeping highlight.
const SHIMMER_WIDTH: f32 = 12.0;

/// Alpha value (0-255) for the white shimmer overlay.
/// Tuned to be visible but not overpowering against the fill color.
/// 76/255 ≈ 30% opacity.
const SHIMMER_ALPHA: u8 = 76;

// -----------------------------------------------------------------------------
// Infinity Sign Animation
// -----------------------------------------------------------------------------

/// Number of line segments used to draw the infinity curve.
/// Higher values = smoother curve but more draw calls.
const NUM_POINTS: usize = 32;

/// Length of the fading trail behind the infinity animation head.
/// Expressed as a fraction of the full loop (0.0-1.0).
const TRAIL_LENGTH: f32 = 0.35;

/// Schedule the next animation frame.
///
/// Call this once per frame when any animation is visible.
/// Multiple calls per frame are harmless but wasteful.
#[inline]
pub fn schedule_frame(ctx: &egui::Context) {
    ctx.request_repaint_after(std::time::Duration::from_millis(FRAME_INTERVAL_MS));
}

/// Compute the current glow intensity for the completed session pulse animation.
///
/// Returns an alpha value (`f32` in `0.0..=1.0`) representing the current pulse
/// intensity, oscillating smoothly between a subtle glow (~0.2) and a bright glow
/// (~0.7) on a ~2-second ease-in-out cycle.
///
/// This is a pure computation with no side effects. Callers are responsible for
/// calling [`schedule_frame`] to keep the animation running.
///
/// # Arguments
/// * `time` - Current animation time in seconds (from `ui.ctx().input(|i| i.time)`)
#[inline]
pub fn completed_glow_intensity(time: f64) -> f32 {
    let phase = (time * std::f64::consts::TAU / COMPLETED_GLOW_PERIOD).cos();
    let t = ((1.0 - phase) * 0.5) as f32;
    COMPLETED_GLOW_ALPHA_MIN + (COMPLETED_GLOW_ALPHA_MAX - COMPLETED_GLOW_ALPHA_MIN) * t
}

/// Render rising particles animation.
///
/// Particles rise from the bottom center and spread outward as they rise,
/// fading out as they reach the top.
///
/// Note: This is a pure render function. Call `schedule_frame()` separately.
pub fn render_rising_particles(ui: &mut Ui, width: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    let painter = ui.painter();
    let time = ui.ctx().input(|i| i.time) as f32;

    // Particle configs: x_center (0-1, where 0.5 is center), spread_amount, size, color, speed, phase
    // Particles start near center and spread outward as they rise
    const PARTICLES: [(f32, f32, f32, Color32, f32, f32); 8] = [
        (0.35, 0.4, 4.5, Color32::from_rgb(255, 150, 170), 0.75, 0.0),
        (
            0.45,
            0.35,
            5.0,
            Color32::from_rgb(200, 140, 220),
            1.1,
            0.125,
        ),
        (0.55, 0.3, 4.0, Color32::from_rgb(180, 160, 255), 0.9, 0.25),
        (
            0.50,
            0.45,
            5.5,
            Color32::from_rgb(240, 170, 210),
            1.25,
            0.375,
        ),
        (0.40, 0.5, 4.8, Color32::from_rgb(170, 200, 230), 0.8, 0.5),
        (
            0.60,
            0.4,
            4.2,
            Color32::from_rgb(220, 180, 200),
            1.15,
            0.625,
        ),
        (
            0.48,
            0.35,
            5.0,
            Color32::from_rgb(190, 150, 240),
            0.85,
            0.75,
        ),
        (0.52, 0.5, 4.5, Color32::from_rgb(255, 170, 190), 1.0, 0.875),
    ];

    for (x_center, spread, dot_size, color, speed, phase) in PARTICLES.iter() {
        let cycle = ((time * 0.1 * speed) + phase) % 1.0;
        let y_progress = cycle;

        // Start near center, spread outward as particle rises
        // spread_amount controls how far from center the particle drifts
        let spread_direction = if *x_center < 0.5 { -1.0 } else { 1.0 };
        let x_offset = spread_direction * spread * y_progress;
        let x_pct = x_center + x_offset;

        let x = rect.left() + width * x_pct;
        let y = rect.bottom() - height * y_progress;

        let alpha = if y_progress < 0.2 {
            y_progress / 0.2
        } else if y_progress > 0.7 {
            1.0 - (y_progress - 0.7) / 0.3
        } else {
            1.0
        };

        // Slight horizontal drift for organic feel
        let drift = (time * 0.5 + phase * 10.0).sin() * PARTICLE_DRIFT;
        let final_x = x + drift;

        painter.circle_filled(
            egui::pos2(final_x, y),
            *dot_size,
            color.linear_multiply(alpha.max(0.0) * PARTICLE_ALPHA_MULTIPLIER),
        );
    }
}

/// Compute a point on the unit lemniscate for parameter t (0 to TAU).
#[inline]
fn lemniscate_point(t: f32) -> (f32, f32) {
    let sin_t = t.sin();
    let cos_t = t.cos();
    let denom = 1.0 + sin_t * sin_t;
    (cos_t / denom, sin_t * cos_t / denom)
}

/// Render a self-drawing infinity sign animation.
///
/// The infinity symbol traces itself continuously in a smooth loop,
/// with a gradient trail effect that fades out behind the leading edge.
///
/// Note: This is a pure render function. Call `schedule_frame()` separately.
///
/// # Arguments
/// * `painter` - The egui painter to draw with
/// * `time` - Current animation time in seconds
/// * `rect` - The rectangle to draw the infinity sign within
/// * `color` - The primary color for the infinity sign
/// * `speed` - Animation speed multiplier (1.0 = normal speed)
pub fn render_infinity(painter: &egui::Painter, time: f32, rect: Rect, color: Color32, speed: f32) {
    let center = rect.center();
    let half_width = rect.width() / 2.0 - 2.0;
    let half_height = rect.height() / 2.0 - 1.0;

    // Animation cycle: 0.0 to 1.0 over the full loop
    let cycle = (time * speed * 0.3) % 1.0;

    let head_pos = cycle;
    let tail_pos = (cycle - TRAIL_LENGTH).rem_euclid(1.0);

    // Only draw segments in the visible trail range
    for i in 0..NUM_POINTS {
        let seg_mid = (i as f32 + 0.5) / NUM_POINTS as f32;

        // Check if this segment is within the trail
        let in_trail = if head_pos > tail_pos {
            seg_mid >= tail_pos && seg_mid <= head_pos
        } else {
            seg_mid >= tail_pos || seg_mid <= head_pos
        };

        if !in_trail {
            continue;
        }

        // Calculate distance from head position (accounting for wraparound)
        let dist_from_head = {
            let direct = (head_pos - seg_mid).abs();
            let wrapped = 1.0 - direct;
            direct.min(wrapped)
        };

        // Alpha based on distance from head
        let alpha = if dist_from_head < TRAIL_LENGTH {
            let progress = 1.0 - (dist_from_head / TRAIL_LENGTH);
            progress * progress
        } else {
            0.0
        };

        if alpha > 0.02 {
            // Compute points on demand
            let t1 = (i as f32 / NUM_POINTS as f32) * std::f32::consts::TAU;
            let t2 = ((i + 1) as f32 / NUM_POINTS as f32) * std::f32::consts::TAU;
            let (x1, y1) = lemniscate_point(t1);
            let (x2, y2) = lemniscate_point(t2);

            let p1 = egui::pos2(center.x + half_width * x1, center.y + half_height * y1);
            let p2 = egui::pos2(center.x + half_width * x2, center.y + half_height * y2);

            painter.line_segment([p1, p2], Stroke::new(1.5, color.linear_multiply(alpha)));
        }
    }

    // Draw head dot
    let head_t = cycle * std::f32::consts::TAU;
    let (hx, hy) = lemniscate_point(head_t);
    painter.circle_filled(
        egui::pos2(center.x + half_width * hx, center.y + half_height * hy),
        2.0,
        color,
    );
}

/// Render a horizontal progress bar with animated shimmer effect.
///
/// The bar fills from left to right based on progress, with a subtle
/// shimmer highlight that sweeps across the filled portion.
///
/// Note: This is a pure render function. Call `schedule_frame()` separately.
///
/// # Arguments
/// * `painter` - The egui painter to draw with
/// * `time` - Current animation time in seconds
/// * `rect` - The rectangle for the progress bar
/// * `progress` - Progress value from 0.0 to 1.0
/// * `bg_color` - Background (unfilled) color
/// * `fill_color` - Fill (progress) color
pub fn render_progress_bar(
    painter: &egui::Painter,
    time: f32,
    rect: Rect,
    progress: f32,
    bg_color: Color32,
    fill_color: Color32,
) {
    let progress = progress.clamp(0.0, 1.0);
    let rounding = Rounding::same(rect.height() / 2.0);

    // Draw background track
    painter.rect_filled(rect, rounding, bg_color);

    if progress > 0.0 {
        // Calculate filled portion (fills from left to right)
        let fill_width = rect.width() * progress;
        let fill_rect =
            Rect::from_min_max(rect.min, egui::pos2(rect.min.x + fill_width, rect.max.y));
        painter.rect_filled(fill_rect, rounding, fill_color);

        // Animated shimmer effect - a bright highlight that sweeps across
        let shimmer_cycle = (time * 0.5) % 2.0; // Slower cycle with pause
        if shimmer_cycle < 1.0 {
            let shimmer_pos = shimmer_cycle;
            let shimmer_x = fill_rect.min.x + (fill_width * shimmer_pos);

            // Only draw shimmer if it's within the filled area
            if shimmer_x >= fill_rect.min.x && shimmer_x <= fill_rect.max.x {
                let shimmer_width = SHIMMER_WIDTH.min(fill_width * 0.3);
                let shimmer_rect = Rect::from_min_max(
                    egui::pos2(
                        (shimmer_x - shimmer_width / 2.0).max(fill_rect.min.x),
                        rect.min.y,
                    ),
                    egui::pos2(
                        (shimmer_x + shimmer_width / 2.0).min(fill_rect.max.x),
                        rect.max.y,
                    ),
                );

                let shimmer_color = Color32::from_rgba_unmultiplied(255, 255, 255, SHIMMER_ALPHA);
                painter.rect_filled(shimmer_rect, rounding, shimmer_color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_glow_stays_in_range() {
        // Sample many points across several cycles
        for i in 0..1000 {
            let time = i as f64 * 0.01;
            let alpha = completed_glow_intensity(time);
            assert!(
                alpha >= COMPLETED_GLOW_ALPHA_MIN && alpha <= COMPLETED_GLOW_ALPHA_MAX,
                "alpha {alpha} out of range at time {time}"
            );
        }
    }

    #[test]
    fn completed_glow_hits_extremes() {
        // At time=0 cosine is 1, so t=0 → alpha_min
        let alpha_at_zero = completed_glow_intensity(0.0);
        assert!(
            (alpha_at_zero - COMPLETED_GLOW_ALPHA_MIN).abs() < 1e-5,
            "expected min at t=0, got {alpha_at_zero}"
        );

        // At time=period/2, cosine is -1, so t=1 → alpha_max
        let alpha_at_half = completed_glow_intensity(COMPLETED_GLOW_PERIOD / 2.0);
        assert!(
            (alpha_at_half - COMPLETED_GLOW_ALPHA_MAX).abs() < 1e-5,
            "expected max at t=period/2, got {alpha_at_half}"
        );
    }

    #[test]
    fn completed_glow_is_periodic() {
        let t = 0.37;
        let alpha1 = completed_glow_intensity(t);
        let alpha2 = completed_glow_intensity(t + COMPLETED_GLOW_PERIOD);
        assert!(
            (alpha1 - alpha2).abs() < 1e-5,
            "expected periodic: {alpha1} vs {alpha2}"
        );
    }

    #[test]
    fn completed_glow_is_smooth() {
        // Check that adjacent samples don't jump too much (smooth, not stepped)
        let dt = 0.001;
        let max_delta = (COMPLETED_GLOW_ALPHA_MAX - COMPLETED_GLOW_ALPHA_MIN)
            * std::f32::consts::PI
            * (dt as f32 / COMPLETED_GLOW_PERIOD as f32);
        for i in 0..2000 {
            let t = i as f64 * dt;
            let a1 = completed_glow_intensity(t);
            let a2 = completed_glow_intensity(t + dt);
            let delta = (a2 - a1).abs();
            assert!(
                delta <= max_delta + 1e-5,
                "jump too large at t={t}: delta={delta}, max={max_delta}"
            );
        }
    }
}
