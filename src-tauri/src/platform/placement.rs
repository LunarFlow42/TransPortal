//! Portable overlay placement: pick a position next to the selection or the
//! cursor inside a monitor work area.

use super::selection::SelectionBounds;

/// Pure calculation of target position given monitor work area, window size, scale factor, anchor bounds or cursor, and center flag.
pub fn compute_smart_placement(
    work: (i32, i32, i32, i32), // (left, top, right, bottom)
    win_size: (i32, i32),       // (width, height)
    scale_factor: f64,
    anchor_bounds: Option<&SelectionBounds>,
    cursor_pos: Option<(i32, i32)>,
    center: bool,
) -> (i32, i32) {
    let (work_left, work_top, work_right, work_bottom) = work;
    let (win_w, win_h) = win_size;
    let margin = (8.0 * scale_factor).round() as i32;
    let offset = (12.0 * scale_factor).round() as i32;

    let (target_x, target_y) = if center {
        let x = work_left + ((work_right - work_left - win_w) / 2);
        let y = work_top + ((work_bottom - work_top - win_h) / 2);
        (x, y)
    } else if let Some(bounds) = anchor_bounds {
        let ideal_x = bounds.left;
        let bottom_y = bounds.top + bounds.height + offset;
        let top_y = bounds.top - win_h - offset;

        let chosen_y = if bottom_y + win_h <= work_bottom - margin {
            bottom_y
        } else if top_y >= work_top + margin {
            top_y
        } else {
            let space_below = work_bottom - bottom_y;
            let space_above = bounds.top - work_top;
            if space_below >= space_above {
                bottom_y
            } else {
                top_y
            }
        };
        (ideal_x, chosen_y)
    } else {
        let (cx, cy) = cursor_pos.unwrap_or((work_left + margin, work_top + margin));
        let cursor_offset_x = (12.0 * scale_factor).round() as i32;
        let cursor_offset_y = (16.0 * scale_factor).round() as i32;

        let ideal_x = cx + cursor_offset_x;
        let chosen_x = if ideal_x + win_w > work_right - margin
            && (cx - win_w - cursor_offset_x) >= work_left + margin
        {
            cx - win_w - cursor_offset_x
        } else {
            ideal_x
        };

        let bottom_y = cy + cursor_offset_y;
        let top_y = cy - win_h - cursor_offset_y;
        let chosen_y = if bottom_y + win_h <= work_bottom - margin {
            bottom_y
        } else if top_y >= work_top + margin {
            top_y
        } else {
            bottom_y
        };
        (chosen_x, chosen_y)
    };

    let clamped_x = target_x.clamp(
        work_left + margin,
        (work_right - win_w - margin).max(work_left + margin),
    );
    let clamped_y = target_y.clamp(
        work_top + margin,
        (work_bottom - win_h - margin).max(work_top + margin),
    );

    (clamped_x, clamped_y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_smart_placement_center() {
        let work = (0, 0, 1920, 1040); // 1040 due to taskbar
        let win_size = (420, 500);
        let (x, y) = compute_smart_placement(work, win_size, 1.0, None, None, true);
        assert_eq!(x, (1920 - 420) / 2);
        assert_eq!(y, (1040 - 500) / 2);
    }

    #[test]
    fn test_compute_smart_placement_below_text() {
        let work = (0, 0, 1920, 1040);
        let win_size = (400, 300);
        let bounds = SelectionBounds {
            left: 200,
            top: 100,
            width: 150,
            height: 24,
        };
        let (x, y) = compute_smart_placement(work, win_size, 1.0, Some(&bounds), None, false);
        assert_eq!(x, 200);
        // Default offset is 12px below text (100 + 24 + 12 = 136)
        assert_eq!(y, 136);
    }

    #[test]
    fn test_compute_smart_placement_flips_above_when_near_bottom() {
        let work = (0, 0, 1920, 1040);
        let win_size = (400, 300);
        // Text near bottom of screen (top: 850, bottom: 874)
        // bottom_y = 874 + 12 = 886. 886 + 300 = 1186 > 1040 - 8 (1032) -> collision!
        // Should flip above: 850 - 300 - 12 = 538.
        let bounds = SelectionBounds {
            left: 200,
            top: 850,
            width: 150,
            height: 24,
        };
        let (x, y) = compute_smart_placement(work, win_size, 1.0, Some(&bounds), None, false);
        assert_eq!(x, 200);
        assert_eq!(y, 538);
    }

    #[test]
    fn test_compute_smart_placement_clamps_right_edge() {
        let work = (0, 0, 1920, 1040);
        let win_size = (400, 300);
        // Text near right edge (left: 1700, width: 200)
        let bounds = SelectionBounds {
            left: 1700,
            top: 200,
            width: 200,
            height: 24,
        };
        let (x, y) = compute_smart_placement(work, win_size, 1.0, Some(&bounds), None, false);
        // work.right - win_w - margin = 1920 - 400 - 8 = 1512
        assert_eq!(x, 1512);
        assert_eq!(y, 236);
    }

    #[test]
    fn test_compute_smart_placement_secondary_monitor() {
        // Secondary monitor positioned to the right: left = 1920, right = 3840
        let work = (1920, 0, 3840, 1080);
        let win_size = (400, 300);
        let bounds = SelectionBounds {
            left: 2100,
            top: 300,
            width: 100,
            height: 20,
        };
        let (x, y) = compute_smart_placement(work, win_size, 1.0, Some(&bounds), None, false);
        assert_eq!(x, 2100);
        assert_eq!(y, 332);
    }
}
