//! One-shot frame selection. A new capture session can start with an old but
//! perfectly non-black presentation. Pixel comparisons cannot establish age.

use super::desktop::{ScreenRect, WindowProbeInfo};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

pub(super) const POOL_SIZE: usize = 3;
const FIRST_FRAME_WAIT: Duration = Duration::from_millis(300);
const UPDATE_WAIT: Duration = Duration::from_millis(120);

pub(super) fn fullscreen_target(
    fg: &WindowProbeInfo,
    monitor: ScreenRect,
    region: ScreenRect,
) -> bool {
    // Physical edges, not the work area, process name or the size of the snip.
    // An invisible resize border can extend beyond the monitor. Allow small
    // inward rounding at fractional scales, too.
    let edges = |r: ScreenRect| {
        let (x, y) = (i64::from(r.x), i64::from(r.y));
        [x, y, x + i64::from(r.width), y + i64::from(r.height)]
    };
    let [l, t, r, b] = edges(fg.rect);
    let [ml, mt, mr, mb] = edges(monitor);
    fg.is_visible
        && !fg.is_iconic
        && !matches!(fg.class_name.as_str(), "Progman" | "WorkerW")
        && monitor.contains(&region)
        && l <= ml + 2
        && t <= mt + 2
        && r >= mr - 2
        && b >= mb - 2
}

/// Desktop Duplication sets LastPresentTime to zero on pointer-only updates.
pub(super) fn usable_present(last_present: i64, cutoff: Option<i64>) -> bool {
    cutoff.is_none_or(|t| last_present > 0 && last_present >= t)
}

#[derive(Clone, Copy)]
pub(super) enum FramePolicy {
    First,
    /// A static desktop need not produce another monitor frame.
    PreferUpdate,
    /// Discard the warm-up frame. WGC timestamps are QPC in 100 ns units.
    /// A missing cutoff still permits ordering frames within the same session.
    GameWindow {
        since: Option<i64>,
    },
    /// An independently opened monitor item can also supply a paused scene,
    /// but never return its warm-up image if it predates this snip.
    GameMonitor {
        since: Option<i64>,
    },
}

pub(super) struct Picked<T> {
    pub frame: T,
    pub frame0_latency_ms: u64,
    pub frame1_latency_ms: Option<u64>,
    pub frames_drained: u32,
}

pub(super) fn pick<T>(
    rx: &Receiver<T>,
    policy: FramePolicy,
    stamp: impl Fn(&T) -> Option<i64>,
) -> Result<Picked<T>, String> {
    let start = Instant::now();
    let first = rx
        .recv_timeout(FIRST_FRAME_WAIT)
        .map_err(|e| format!("等待 WGC 首帧失败：{e}"))?;
    let received = Instant::now();
    let frame0_latency_ms = received.duration_since(start).as_millis() as u64;
    let initial = Picked {
        frame: first,
        frame0_latency_ms,
        frame1_latency_ms: None,
        frames_drained: 0,
    };
    if matches!(policy, FramePolicy::First) {
        return Ok(initial);
    }

    let baseline = stamp(&initial.frame);
    // Release the game warm-up surface back to the small pool before waiting.
    let allow_still = match policy {
        FramePolicy::PreferUpdate => true,
        FramePolicy::GameMonitor { since: Some(s) } => baseline.is_some_and(|t| t >= s),
        _ => false,
    };
    let fallback = allow_still.then_some(initial);
    let advances = |time: Option<i64>, previous: Option<i64>| match policy {
        FramePolicy::GameWindow { since } | FramePolicy::GameMonitor { since } => {
            time.is_some_and(|t| {
                t > 0
                    && (previous.is_some() || since.is_some())
                    && previous.is_none_or(|p| t > p)
                    && since.is_none_or(|s| t >= s)
            })
        }
        _ => time.zip(previous).is_none_or(|(t, p)| t > p),
    };
    let mut skipped = 1;
    let deadline = received + UPDATE_WAIT;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Ok(mut frame) = rx.recv_timeout(left) else {
            break;
        };
        let mut time = stamp(&frame);
        if !advances(time, baseline) {
            skipped += 1;
            continue;
        }
        let frame1_latency_ms = Some(received.elapsed().as_millis() as u64);
        // Take the newest *already queued* presentation without chasing a
        // high-refresh producer indefinitely or regressing on reordered events.
        for _ in 0..POOL_SIZE {
            let Ok(next) = rx.try_recv() else { break };
            let next_time = stamp(&next);
            if advances(next_time, time) {
                frame = next;
                time = next_time;
            }
            skipped += 1;
        }
        return Ok(Picked {
            frame,
            frame0_latency_ms,
            frame1_latency_ms,
            frames_drained: skipped,
        });
    }
    fallback
        .ok_or_else(|| "WGC 只收到初始帧或旧帧，未等到本次截图的新呈现；尝试下一捕获通道。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn first_snip_returns_current_scene_not_a_valid_but_cached_previous_scene() {
        let (tx, rx) = mpsc::channel();
        for frame in [
            (80, "previous"),
            (90, "still before hotkey"),
            (100, "current"),
            (106, "latest"),
            (101, "out of order"),
        ] {
            tx.send(frame).unwrap();
        }
        let result = pick(&rx, FramePolicy::GameWindow { since: Some(100) }, |f| {
            Some(f.0)
        })
        .unwrap();
        assert_eq!(result.frame.1, "latest");
        assert!(result.frame1_latency_ms.is_some());
        assert_eq!(result.frames_drained, 4);
    }

    #[test]
    fn lone_or_duplicate_game_frame_falls_through_even_if_stamped_after_request() {
        for times in [vec![Some(101)], vec![Some(101), Some(101), None, Some(99)]] {
            let (tx, rx) = mpsc::channel();
            for time in times {
                tx.send(time).unwrap();
            }
            drop(tx);
            assert!(pick(&rx, FramePolicy::GameWindow { since: Some(100) }, |f| *f).is_err());
        }
    }

    #[test]
    fn static_desktop_and_normal_windows_can_still_use_the_first_frame() {
        for policy in [FramePolicy::First, FramePolicy::PreferUpdate] {
            let (tx, rx) = mpsc::channel();
            tx.send(7).unwrap();
            drop(tx);
            let before = Instant::now();
            let result = pick(&rx, policy, |f| Some(*f)).unwrap();
            assert_eq!(result.frame, 7);
            assert!(result.frame1_latency_ms.is_none());
            assert!(u128::from(result.frame0_latency_ms) <= before.elapsed().as_millis());
        }
    }

    #[test]
    fn paused_game_can_use_a_current_monitor_still_but_not_one_from_before_the_snip() {
        for (stamp, expected) in [(120, true), (99, false)] {
            let (tx, rx) = mpsc::channel();
            tx.send(stamp).unwrap(); // keep the channel open, as on a static desktop
            let picked = pick(&rx, FramePolicy::GameMonitor { since: Some(100) }, |f| {
                Some(*f)
            });
            assert_eq!(picked.is_ok(), expected);
            if let Ok(p) = picked {
                assert!(p.frame1_latency_ms.is_none());
                assert_eq!(p.frame, stamp);
            }
        }
    }

    #[test]
    fn only_visible_fullscreen_foreground_on_the_requested_monitor_needs_freshness() {
        let monitor = ScreenRect {
            x: -2560,
            y: -80,
            width: 2560,
            height: 1440,
        };
        let subtitle = ScreenRect {
            x: -1560,
            y: 1200,
            width: 800,
            height: 80,
        };
        let mut fg = WindowProbeInfo {
            hwnd: 1,
            title: String::new(),
            class_name: "Game".into(),
            process_id: 2,
            process_name: String::new(),
            rect: monitor,
            is_visible: true,
            is_iconic: false,
            contains_target: true,
        };
        assert!(fullscreen_target(&fg, monitor, subtitle));
        assert!(!fullscreen_target(
            &fg,
            ScreenRect { x: 0, ..monitor },
            subtitle
        ));
        fg.rect.height -= 40; // maximized to the work area, not fullscreen
        assert!(!fullscreen_target(&fg, monitor, subtitle));
        fg.rect = monitor;
        fg.rect.x -= 8;
        fg.rect.width += 16;
        assert!(fullscreen_target(&fg, monitor, subtitle));
        fg.is_iconic = true;
        assert!(!fullscreen_target(&fg, monitor, subtitle));
    }

    #[test]
    fn duplication_needs_new_desktop_pixels_not_just_a_cursor_update() {
        let chosen = [0, 80, 0, 110]
            .into_iter()
            .find(|t| usable_present(*t, Some(100)));
        assert_eq!(chosen, Some(110));
    }
}
