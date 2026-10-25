//! The native window frame in Focus's colours: DWM paints the title bar, its text and the border
//! to match the theme (Windows 11; older Windows keeps its own frame). Elsewhere this does nothing.

use iced::Theme;

/// Colours the frame of the window `handle` (its HWND, from `iced::window::raw_id`) for `theme`.
#[cfg(windows)]
pub fn paint(handle: u64, theme: &Theme) {
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
    };

    let window = handle as windows_sys::Win32::Foundation::HWND;
    let colours = crate::style::frame_colours(theme);
    // Dark mode also turns the minimise, maximise and close glyphs light.
    let dark = windows_sys::core::BOOL::from(theme.extended_palette().is_dark);
    set(window, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark);
    for (attribute, colour) in [
        (DWMWA_CAPTION_COLOR, colours.caption),
        (DWMWA_TEXT_COLOR, colours.text),
        (DWMWA_BORDER_COLOR, colours.border),
    ] {
        set(window, attribute, &colorref(colour));
    }
}

/// Does nothing: only Windows has DWM.
#[cfg(not(windows))]
pub fn paint(_handle: u64, _theme: &Theme) {}

/// Sets one DWM attribute of `window`. Failures (older Windows, a closed window) are ignored:
/// the frame just keeps its default look.
#[cfg(windows)]
fn set<T>(
    window: windows_sys::Win32::Foundation::HWND,
    attribute: windows_sys::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE,
    value: &T,
) {
    // SAFETY: `value` points at a live `T` and the size passed is its size, which is all the call
    // reads; a bad window handle only makes it return an error.
    unsafe {
        windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
            window,
            attribute as u32,
            (value as *const T).cast(),
            size_of::<T>() as u32,
        );
    }
}

/// `colour` as a Win32 COLORREF (0x00BBGGRR).
#[cfg(windows)]
fn colorref(colour: iced::Color) -> u32 {
    let [red, green, blue, _] = colour.into_rgba8();
    u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn colorref_puts_red_lowest() {
        assert_eq!(
            colorref(iced::Color::from_rgb8(0x12, 0x34, 0x56)),
            0x0056_3412
        );
    }
}
