//! Decorative animations for the GUI.
//!
//! Animation functions are pure renderers - they don't schedule repaints.
//! Call `schedule_frame()` once per frame when any animation is visible.

use egui::{Color32, Rect, Rounding, Sense, Stroke, Ui};

/// Animation frame interval (~30fps for smooth animation with low CPU).
const FRAME_INTERVAL_MS: u64 = 33;

/// Schedule the next animation frame.
///
/// Call this once per frame when any animation is visible.
/// Multiple calls per frame are harmless but wasteful.
#[inline]
pub fn schedule_frame(ctx: &egui::Context) {
    ctx.request_repaint_after(std::time::Duration::from_millis(FRAME_INTERVAL_MS));
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
        let drift = (time * 0.5 + phase * 10.0).sin() * 6.0;
        let final_x = x + drift;

        painter.circle_filled(
            egui::pos2(final_x, y),
            *dot_size,
            color.linear_multiply(alpha.max(0.0) * 0.7),
        );
    }
}

/// Precomputed infinity path points (unit lemniscate, centered at origin).
/// Generated once at compile time to avoid per-frame allocations.
const NUM_POINTS: usize = 32;

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

    const TRAIL_LENGTH: f32 = 0.35;
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

/// Render a subtle sparkle/twinkle effect for the mascot's hat.
///
/// Draws small star-like glints that fade in and out at different rates,
/// creating a gentle twinkling effect. The sparkles are positioned randomly
/// within the given rect and cycle smoothly without visible jumps.
///
/// Note: This is a pure render function. Call `schedule_frame()` separately.
///
/// # Arguments
/// * `painter` - The egui painter to draw with
/// * `time` - Current animation time in seconds
/// * `rect` - The rectangle to draw sparkles within (roughly 30-40px area)
/// * `color` - The base color for the sparkles
pub fn render_hat_sparkle(painter: &egui::Painter, time: f32, rect: Rect, color: Color32) {
    let center = rect.center();
    let half_width = rect.width() / 2.0;
    let half_height = rect.height() / 2.0;

    // Sparkle configurations: (x_offset, y_offset, size, speed, phase)
    // Offsets are relative to center (-1.0 to 1.0 range)
    // Each sparkle has different timing to create varied twinkling
    const SPARKLES: [(f32, f32, f32, f32, f32); 5] = [
        (0.0, -0.3, 3.0, 1.0, 0.0),  // Center-top, medium size
        (-0.5, 0.1, 2.0, 1.4, 0.25), // Left, small
        (0.6, -0.1, 2.5, 0.9, 0.5),  // Right, medium-small
        (-0.2, 0.4, 1.8, 1.2, 0.7),  // Lower-left, tiny
        (0.3, 0.3, 2.2, 1.1, 0.15),  // Lower-right, small
    ];

    for (x_off, y_off, size, speed, phase) in SPARKLES.iter() {
        // Smooth fade cycle using sine wave for seamless looping
        // Each sparkle fades in and out independently
        let cycle = ((time * speed * 0.8) + phase * std::f32::consts::TAU).sin();
        // Map from [-1, 1] to [0, 1] for alpha, with extra power for more "off" time
        let alpha = ((cycle + 1.0) / 2.0).powf(2.0);

        if alpha < 0.05 {
            continue; // Skip nearly invisible sparkles
        }

        let x = center.x + half_width * x_off;
        let y = center.y + half_height * y_off;
        let pos = egui::pos2(x, y);

        // Draw a 4-pointed star shape for each sparkle
        let arm_length = size * (0.8 + alpha * 0.4); // Slightly larger when brighter
        let sparkle_color = color.linear_multiply(alpha * 0.9);

        // Vertical arm
        painter.line_segment(
            [
                egui::pos2(pos.x, pos.y - arm_length),
                egui::pos2(pos.x, pos.y + arm_length),
            ],
            Stroke::new(1.0, sparkle_color),
        );

        // Horizontal arm
        painter.line_segment(
            [
                egui::pos2(pos.x - arm_length, pos.y),
                egui::pos2(pos.x + arm_length, pos.y),
            ],
            Stroke::new(1.0, sparkle_color),
        );

        // Small center dot for extra brightness at peak
        if alpha > 0.5 {
            let dot_alpha = (alpha - 0.5) * 2.0; // 0 to 1 as alpha goes 0.5 to 1
            painter.circle_filled(pos, size * 0.3, color.linear_multiply(dot_alpha * 0.7));
        }
    }
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
                let shimmer_width = 12.0_f32.min(fill_width * 0.3);
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

                let shimmer_color = Color32::from_rgba_unmultiplied(255, 255, 255, 76);
                painter.rect_filled(shimmer_rect, rounding, shimmer_color);
            }
        }
    }
}
