//! Decorative animations for the GUI.
//!
//! To disable animations, simply don't call the render function.

use egui::{Color32, Sense, Ui};

/// Render rising particles animation.
///
/// Particles rise from the bottom center and spread outward as they rise,
/// fading out as they reach the top.
pub fn render_rising_particles(ui: &mut Ui, width: f32, height: f32) {
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(33));

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
