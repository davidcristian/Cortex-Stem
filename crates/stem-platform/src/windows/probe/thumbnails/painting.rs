//! What the destination and the source windows paint.

use std::cell::RefCell;

use windows::Win32::Foundation::{COLORREF, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, DT_CENTER, DT_LEFT, EndPaint, FW_NORMAL, FW_SEMIBOLD, HDC, PAINTSTRUCT, SetBkMode,
    TRANSPARENT,
};

use super::gdi::{GdiObject, client_rect, fill, outline, single_line, text};
use super::geometry::{height, inflate, px};
use super::quit::START;
use super::{BACKGROUND, OUTLINE, SLOT_EDGE, SLOT_FILL, SLOT_TEXT, TEXT, TEXT_DIM, WHITE};

/// What the destination paints; filled by the probe, read by `WM_PAINT`.
#[derive(Debug)]
pub(super) struct Board {
    pub(super) scale: f64,
    pub(super) header: String,
    pub(super) legend: String,
    pub(super) slots: Vec<BoardSlot>,
}

impl Board {
    const fn empty() -> Self {
        Self {
            scale: 1.0,
            header: String::new(),
            legend: String::new(),
            slots: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub(super) struct BoardSlot {
    pub(super) label: RECT,
    pub(super) image: RECT,
    pub(super) title: String,
    pub(super) status: String,
    pub(super) placed: Option<RECT>,
}

/// What a source window paints.
#[derive(Debug)]
pub(super) struct SourcePaint {
    pub(super) hwnd: isize,
    pub(super) number: usize,
    pub(super) key: &'static str,
    pub(super) colour: COLORREF,
    pub(super) scale: f64,
}

thread_local! {
    pub(super) static BOARD: RefCell<Board> = const { RefCell::new(Board::empty()) };
    pub(super) static SOURCES: RefCell<Vec<SourcePaint>> = const { RefCell::new(Vec::new()) };
}

fn draw_board(hdc: HDC, board: &Board, client: RECT) {
    fill(hdc, client, BACKGROUND);
    let scale = board.scale;
    let header_font = GdiObject::font(px(15.0, scale), FW_SEMIBOLD);
    let label_font = GdiObject::font(px(13.0, scale), FW_NORMAL);
    // SAFETY: `hdc` is the DC being painted.
    let _ = unsafe { SetBkMode(hdc, TRANSPARENT) };
    let line = px(18.0, scale);
    let header = RECT {
        left: client.left + px(12.0, scale),
        top: client.top + px(6.0, scale),
        right: client.right - px(12.0, scale),
        bottom: client.top + px(6.0, scale) + line,
    };
    text(
        hdc,
        header,
        &board.header,
        TEXT,
        header_font.as_ref(),
        single_line(DT_LEFT),
    );
    let legend = RECT {
        top: header.bottom,
        bottom: header.bottom + line,
        ..header
    };
    text(
        hdc,
        legend,
        &board.legend,
        TEXT_DIM,
        label_font.as_ref(),
        single_line(DT_LEFT),
    );
    for slot in &board.slots {
        let first = RECT {
            bottom: slot.label.top + line,
            ..slot.label
        };
        let second = RECT {
            top: first.bottom,
            bottom: first.bottom + line,
            ..slot.label
        };
        text(
            hdc,
            first,
            &slot.title,
            TEXT,
            header_font.as_ref(),
            single_line(DT_LEFT),
        );
        text(
            hdc,
            second,
            &slot.status,
            TEXT_DIM,
            label_font.as_ref(),
            single_line(DT_LEFT),
        );
        fill(hdc, slot.image, SLOT_FILL);
        text(
            hdc,
            slot.image,
            "no picture here",
            SLOT_TEXT,
            label_font.as_ref(),
            single_line(DT_CENTER),
        );
        outline(hdc, inflate(slot.image, 1), SLOT_EDGE);
        if let Some(placed) = slot.placed {
            outline(hdc, inflate(placed, 1), OUTLINE);
            outline(hdc, inflate(placed, 2), OUTLINE);
        }
    }
}

pub(super) fn paint_board(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    // SAFETY: `hwnd` is the window being painted, on its own thread;
    // `paint` is a live out-struct.
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    if !hdc.is_invalid() {
        let client = client_rect(hwnd);
        let _ = BOARD.try_with(|board| {
            if let Ok(board) = board.try_borrow() {
                draw_board(hdc, &board, client);
            }
        });
    }
    // SAFETY: balances the BeginPaint above with the same structure.
    let _ = unsafe { EndPaint(hwnd, &paint) };
}

pub(super) fn paint_source(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    // SAFETY: `hwnd` is the window being painted, on its own thread;
    // `paint` is a live out-struct.
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    let id = hwnd.0 as isize;
    let found = SOURCES
        .try_with(|sources| {
            sources.try_borrow().ok().and_then(|list| {
                list.iter()
                    .find(|source| source.hwnd == id)
                    .map(|source| (source.number, source.key, source.colour, source.scale))
            })
        })
        .ok()
        .flatten();
    if let Some((number, key, colour, scale)) = found
        && !hdc.is_invalid()
    {
        let client = client_rect(hwnd);
        fill(hdc, client, WHITE);
        fill(hdc, inflate(client, -px(4.0, scale)), colour);
        // SAFETY: `hdc` is the DC being painted.
        let _ = unsafe { SetBkMode(hdc, TRANSPARENT) };
        let big = GdiObject::font(px(44.0, scale), FW_SEMIBOLD);
        let small = GdiObject::font(px(26.0, scale), FW_NORMAL);
        let elapsed = START
            .get()
            .map_or(0.0, |start| start.elapsed().as_secs_f64());
        let middle = client.top + height(client) / 2;
        let upper = RECT {
            bottom: middle,
            ..client
        };
        let lower = RECT {
            top: middle,
            ..client
        };
        text(
            hdc,
            upper,
            &format!("{number} {key}"),
            WHITE,
            big.as_ref(),
            single_line(DT_CENTER),
        );
        text(
            hdc,
            lower,
            &format!("t = {elapsed:.1} s"),
            WHITE,
            small.as_ref(),
            single_line(DT_CENTER),
        );
    }
    // SAFETY: balances the BeginPaint above with the same structure.
    let _ = unsafe { EndPaint(hwnd, &paint) };
}
