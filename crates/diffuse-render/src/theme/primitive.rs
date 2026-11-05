//! Foundational color swatches and symbolic palette resolution.
//!
//! Provides a standardized set of primitive swatches used for baseline
//! fallbacks and variable expansion (`$red`, `$green`, etc.) in declarative
//! themes.

use yansi::Color;

/// Foundational primitive color swatches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimitivePalette {
    pub base_bg: Color,
    pub base_fg: Color,
    pub red: Color,
    pub green: Color,
    pub yellow: Color,
    pub orange: Color,
    pub blue: Color,
    pub purple: Color,
    pub cyan: Color,
    pub gray: Color,
    pub dark_red: Color,
    pub dark_green: Color,
}

impl PrimitivePalette {
    /// Canonical dark theme foundation swatches.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            base_bg: Color::Fixed(233),
            base_fg: Color::Fixed(231),
            red: Color::Fixed(203),
            green: Color::Fixed(120),
            yellow: Color::Fixed(220),
            orange: Color::Fixed(214),
            blue: Color::Fixed(117),
            purple: Color::Fixed(147),
            cyan: Color::Fixed(51),
            gray: Color::Fixed(245),
            dark_red: Color::Rgb(38, 12, 14),
            dark_green: Color::Rgb(28, 50, 30),
        }
    }

    /// Canonical light theme foundation swatches.
    #[must_use]
    pub const fn light() -> Self {
        Self {
            base_bg: Color::Fixed(255),
            base_fg: Color::Rgb(36, 41, 47),
            red: Color::Fixed(160),
            green: Color::Fixed(28),
            yellow: Color::Fixed(172),
            orange: Color::Fixed(166),
            blue: Color::Fixed(31),
            purple: Color::Fixed(97),
            cyan: Color::Fixed(37),
            gray: Color::Fixed(244),
            dark_red: Color::Rgb(254, 238, 240),
            dark_green: Color::Rgb(235, 247, 238),
        }
    }

    /// Resolve a primitive swatch by variable name (e.g. `"red"`, `"base_bg"`).
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Color> {
        match name {
            "base_bg" => Some(self.base_bg),
            "base_fg" => Some(self.base_fg),
            "red" => Some(self.red),
            "green" => Some(self.green),
            "yellow" => Some(self.yellow),
            "orange" => Some(self.orange),
            "blue" => Some(self.blue),
            "purple" => Some(self.purple),
            "cyan" => Some(self.cyan),
            "gray" => Some(self.gray),
            "dark_red" => Some(self.dark_red),
            "dark_green" => Some(self.dark_green),
            _ => None,
        }
    }
}

impl Default for PrimitivePalette {
    #[inline]
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primitive_palette_presets() {
        let dark = PrimitivePalette::dark();
        assert_eq!(dark.base_bg, Color::Fixed(233));
        assert_eq!(dark.base_fg, Color::Fixed(231));
        assert_eq!(dark.red, Color::Fixed(203));
        assert_eq!(dark.green, Color::Fixed(120));

        let light = PrimitivePalette::light();
        assert_eq!(light.base_bg, Color::Fixed(255));
        assert_eq!(light.base_fg, Color::Rgb(36, 41, 47));
        assert_eq!(light.red, Color::Fixed(160));
        assert_eq!(light.green, Color::Fixed(28));

        let def = PrimitivePalette::default();
        assert_eq!(def, dark);
    }

    #[test]
    fn test_primitive_palette_get() {
        let p = PrimitivePalette::dark();
        assert_eq!(p.get("base_bg"), Some(p.base_bg));
        assert_eq!(p.get("base_fg"), Some(p.base_fg));
        assert_eq!(p.get("red"), Some(p.red));
        assert_eq!(p.get("green"), Some(p.green));
        assert_eq!(p.get("yellow"), Some(p.yellow));
        assert_eq!(p.get("orange"), Some(p.orange));
        assert_eq!(p.get("blue"), Some(p.blue));
        assert_eq!(p.get("purple"), Some(p.purple));
        assert_eq!(p.get("cyan"), Some(p.cyan));
        assert_eq!(p.get("gray"), Some(p.gray));
        assert_eq!(p.get("dark_red"), Some(p.dark_red));
        assert_eq!(p.get("dark_green"), Some(p.dark_green));
        assert_eq!(p.get("unknown"), None);
        assert_eq!(p.get(""), None);
    }
}
