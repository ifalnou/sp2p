use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;

use crate::core::state::GLOBAL_STATE;

use super::wide;

pub fn draw_heading(hdc: HDC, bold_font: HGDIOBJ, x: i32, y: &mut i32, text: &str) {
    unsafe {
        let old = SelectObject(hdc, bold_font);
        let w = wide(text);
        TextOutW(hdc, x, *y, w.as_ptr(), (w.len() - 1) as i32);
        SelectObject(hdc, old);
    }
    *y += 22;
}

pub fn draw_separator(hdc: HDC, left: i32, right: i32, y: &mut i32) {
    let rc = RECT {
        left,
        top: *y,
        right,
        bottom: *y + 1,
    };
    unsafe { FillRect(hdc, &rc, GetSysColorBrush(COLOR_GRAYTEXT)) };
    *y += 8;
}

pub fn draw_line(hdc: HDC, x: i32, y: i32, text: &str) {
    let w = wide(text);
    unsafe { TextOutW(hdc, x, y, w.as_ptr(), (w.len() - 1) as i32) };
}

pub fn paint_progress(hdc: HDC, bold_font: HGDIOBJ, left: i32, right: i32, y: &mut i32) {
    draw_heading(hdc, bold_font, left, y, "Active Transfers");
    draw_separator(hdc, left, right, y);

    let global = GLOBAL_STATE.read().unwrap();
    if global.active_transfers.is_empty() {
        draw_line(hdc, left, *y, "No active transfers.");
        return;
    }

    let bar_right = right - 90;
    let highlight = unsafe { GetSysColor(COLOR_HIGHLIGHT) };

    for t in &global.active_transfers {
        let dir = if t.is_sending { "Sending" } else { "Receiving" };
        draw_line(hdc, left, *y, &format!("{} \u{2013} {}", dir, t.filename));
        *y += 20;

        unsafe {
            // Progress bar background
            let bg_brush = CreateSolidBrush(0x00E0E0E0);
            let bg = RECT {
                left,
                top: *y,
                right: bar_right,
                bottom: *y + 16,
            };
            FillRect(hdc, &bg, bg_brush);
            DeleteObject(bg_brush);

            // Progress bar fill
            let progress = if t.total_bytes > 0 {
                (t.bytes_transferred as f64) / (t.total_bytes as f64)
            } else {
                0.0
            };
            let fill_w = ((bar_right - left) as f64 * progress) as i32;
            if fill_w > 0 {
                let fill_brush = CreateSolidBrush(highlight);
                let fill = RECT {
                    left,
                    top: *y,
                    right: left + fill_w,
                    bottom: *y + 16,
                };
                FillRect(hdc, &fill, fill_brush);
                DeleteObject(fill_brush);
            }
        }

        // Size label
        draw_line(
            hdc,
            bar_right + 8,
            *y,
            &format!(
                "{}/{} MB",
                t.bytes_transferred / 1_000_000,
                t.total_bytes / 1_000_000
            ),
        );
        *y += 24;
    }
}

pub fn paint_history(hdc: HDC, bold_font: HGDIOBJ, left: i32, right: i32, y: &mut i32) {
    draw_heading(hdc, bold_font, left, y, "Recent History");
    draw_separator(hdc, left, right, y);

    let global = GLOBAL_STATE.read().unwrap();
    let mut history = global.history.clone();
    history.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    if history.is_empty() {
        draw_line(hdc, left, *y, "No recent transfers.");
        return;
    }

    for item in history.iter().take(50) {
        let dir = if item.is_sending { "Sent" } else { "Received" };
        let status = if item.success { "Success" } else { "Failed" };
        draw_line(
            hdc,
            left,
            *y,
            &format!("{} \u{2013} {} ({})", dir, item.filename, status),
        );
        *y += 22;
    }
}

pub fn paint_settings_heading(
    hdc: HDC,
    bold_font: HGDIOBJ,
    left: i32,
    right: i32,
    y: &mut i32,
) {
    draw_heading(hdc, bold_font, left, y, "Application Settings");
    draw_separator(hdc, left, right, y);
}
