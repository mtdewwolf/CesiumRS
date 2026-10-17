//! CesiumJS `Color`: RGBA with components in `[0, 1]`, plus CSS parsing
//! and the CSS named colors.

use super::cartesian4::Cartesian4;
use super::math::{js_number_to_string, lerp, next_random_number, to_int32};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

/// Opaque white, like `new Color()` in Cesium.
impl Default for Color {
    fn default() -> Self {
        Self::WHITE
    }
}

/// Bounds or fixed values for [`Color::from_random`]. A fixed component wins
/// over its bounds; bounds default to `[0, 1]`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ColorRandomOptions {
    pub red: Option<f64>,
    pub minimum_red: Option<f64>,
    pub maximum_red: Option<f64>,
    pub green: Option<f64>,
    pub minimum_green: Option<f64>,
    pub maximum_green: Option<f64>,
    pub blue: Option<f64>,
    pub minimum_blue: Option<f64>,
    pub maximum_blue: Option<f64>,
    pub alpha: Option<f64>,
    pub minimum_alpha: Option<f64>,
    pub maximum_alpha: Option<f64>,
}

fn hue2rgb(m1: f64, m2: f64, mut h: f64) -> f64 {
    if h < 0.0 {
        h += 1.0;
    }
    if h > 1.0 {
        h -= 1.0;
    }
    if h * 6.0 < 1.0 {
        return m1 + (m2 - m1) * 6.0 * h;
    }
    if h * 2.0 < 1.0 {
        return m2;
    }
    if h * 3.0 < 2.0 {
        return m1 + (m2 - m1) * (2.0 / 3.0 - h) * 6.0;
    }
    m1
}

const fn hex(rgb: u32) -> Color {
    Color::new(
        ((rgb >> 16) & 0xff) as f64 / 255.0,
        ((rgb >> 8) & 0xff) as f64 / 255.0,
        (rgb & 0xff) as f64 / 255.0,
        1.0,
    )
}

impl Color {
    pub const PACKED_LENGTH: usize = 4;

    pub const fn new(red: f64, green: f64, blue: f64, alpha: f64) -> Self {
        Self { red, green, blue, alpha }
    }

    pub fn from_cartesian4(c: Cartesian4) -> Self {
        Self::new(c.x, c.y, c.z, c.w)
    }

    /// Components in `[0, 255]`.
    pub fn from_bytes(red: f64, green: f64, blue: f64, alpha: f64) -> Self {
        Self::new(
            Self::byte_to_float(red),
            Self::byte_to_float(green),
            Self::byte_to_float(blue),
            Self::byte_to_float(alpha),
        )
    }

    pub fn from_alpha(color: &Self, alpha: f64) -> Self {
        Self::new(color.red, color.green, color.blue, alpha)
    }

    /// A packed `0xAABBGGRR` value, as read from little-endian RGBA bytes.
    pub fn from_rgba(rgba: u32) -> Self {
        let b = rgba.to_le_bytes();
        Self::from_bytes(f64::from(b[0]), f64::from(b[1]), f64::from(b[2]), f64::from(b[3]))
    }

    /// Hue, saturation, and lightness in `[0, 1]`.
    pub fn from_hsl(hue: f64, saturation: f64, lightness: f64, alpha: f64) -> Self {
        let hue = hue % 1.0;
        let (mut red, mut green, mut blue) = (lightness, lightness, lightness);
        if saturation != 0.0 {
            let m2 = if lightness < 0.5 {
                lightness * (1.0 + saturation)
            } else {
                lightness + saturation - lightness * saturation
            };
            let m1 = 2.0 * lightness - m2;
            red = hue2rgb(m1, m2, hue + 1.0 / 3.0);
            green = hue2rgb(m1, m2, hue);
            blue = hue2rgb(m1, m2, hue - 1.0 / 3.0);
        }
        Self::new(red, green, blue, alpha)
    }

    /// Random color from [`next_random_number`], in red, green, blue, alpha order.
    pub fn from_random(options: &ColorRandomOptions) -> Self {
        let pick = |fixed: Option<f64>, min: Option<f64>, max: Option<f64>| {
            fixed.unwrap_or_else(|| {
                let min = min.unwrap_or(0.0);
                let max = max.unwrap_or(1.0);
                min + next_random_number() * (max - min)
            })
        };
        let red = pick(options.red, options.minimum_red, options.maximum_red);
        let green = pick(options.green, options.minimum_green, options.maximum_green);
        let blue = pick(options.blue, options.minimum_blue, options.maximum_blue);
        let alpha = pick(options.alpha, options.minimum_alpha, options.maximum_alpha);
        Self::new(red, green, blue, alpha)
    }

    /// Parses a CSS color: a named color, `#rgb[a]`, `#rrggbb[aa]`,
    /// `rgb[a](...)`, or `hsl[a](...)`. `None` if it matches none of them.
    pub fn from_css_color_string(color: &str) -> Option<Self> {
        let color = color.trim();
        let upper = color.to_uppercase();
        if let Some(named) = Self::named(&upper) {
            return Some(named);
        }
        if let Some(c) = parse_hex(color) {
            return Some(c);
        }
        let lower = color.to_lowercase();
        if let Some(rest) = lower.strip_prefix("rgba").or_else(|| lower.strip_prefix("rgb")) {
            if let Some((parts, alpha)) = parse_parenthesized(rest, true) {
                let channel = |p: &str| {
                    let percent = p.ends_with('%');
                    js_parse_float(p) / if percent { 100.0 } else { 255.0 }
                };
                return Some(Self::new(
                    channel(parts[0]),
                    channel(parts[1]),
                    channel(parts[2]),
                    alpha.map_or(1.0, js_parse_float),
                ));
            }
        }
        if let Some(rest) = lower.strip_prefix("hsla").or_else(|| lower.strip_prefix("hsl")) {
            if let Some((parts, alpha)) = parse_parenthesized(rest, false) {
                if !parts[0].ends_with('%') && parts[1].ends_with('%') && parts[2].ends_with('%') {
                    return Some(Self::from_hsl(
                        js_parse_float(parts[0]) / 360.0,
                        js_parse_float(parts[1]) / 100.0,
                        js_parse_float(parts[2]) / 100.0,
                        alpha.map_or(1.0, js_parse_float),
                    ));
                }
            }
        }
        None
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        array[starting_index..starting_index + 4].copy_from_slice(&[self.red, self.green, self.blue, self.alpha]);
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        let a = &array[starting_index..starting_index + 4];
        Self::new(a[0], a[1], a[2], a[3])
    }

    pub fn byte_to_float(number: f64) -> f64 {
        number / 255.0
    }

    /// `[0, 1]` to `[0, 255]`, truncating like Cesium's `(n * 256) | 0`.
    pub fn float_to_byte(number: f64) -> i32 {
        if number == 1.0 {
            255
        } else {
            to_int32(number * 256.0)
        }
    }

    pub fn equals(&self, other: &Self) -> bool {
        self == other
    }

    pub fn equals_array(&self, array: &[f64], offset: usize) -> bool {
        self.red == array[offset]
            && self.green == array[offset + 1]
            && self.blue == array[offset + 2]
            && self.alpha == array[offset + 3]
    }

    pub fn equals_epsilon(&self, other: &Self, epsilon: f64) -> bool {
        (self.red - other.red).abs() <= epsilon
            && (self.green - other.green).abs() <= epsilon
            && (self.blue - other.blue).abs() <= epsilon
            && (self.alpha - other.alpha).abs() <= epsilon
    }

    /// `rgb(r,g,b)` when opaque, else `rgba(r,g,b,a)`.
    pub fn to_css_color_string(&self) -> String {
        let (r, g, b) =
            (Self::float_to_byte(self.red), Self::float_to_byte(self.green), Self::float_to_byte(self.blue));
        if self.alpha == 1.0 {
            format!("rgb({r},{g},{b})")
        } else {
            format!("rgba({r},{g},{b},{})", js_number_to_string(self.alpha))
        }
    }

    /// `#rrggbb`, or `#rrggbbaa` when translucent.
    pub fn to_css_hex_string(&self) -> String {
        let h = |v: f64| {
            let b = Self::float_to_byte(v);
            let s = if b < 0 { format!("-{:x}", -i64::from(b)) } else { format!("{b:x}") };
            if s.len() < 2 {
                format!("0{s}")
            } else {
                s
            }
        };
        let rgb = format!("#{}{}{}", h(self.red), h(self.green), h(self.blue));
        if self.alpha < 1.0 {
            format!("{rgb}{}", h(self.alpha))
        } else {
            rgb
        }
    }

    pub fn to_bytes(&self) -> [i32; 4] {
        [
            Self::float_to_byte(self.red),
            Self::float_to_byte(self.green),
            Self::float_to_byte(self.blue),
            Self::float_to_byte(self.alpha),
        ]
    }

    /// Little-endian RGBA bytes as one `u32`; each byte wraps like a `Uint8Array` store.
    pub fn bytes_to_rgba(red: i32, green: i32, blue: i32, alpha: i32) -> u32 {
        u32::from_le_bytes([red as u8, green as u8, blue as u8, alpha as u8])
    }

    pub fn to_rgba(&self) -> u32 {
        let [r, g, b, a] = self.to_bytes();
        Self::bytes_to_rgba(r, g, b, a)
    }

    /// Moves each channel toward 1 by `magnitude`.
    pub fn brighten(&self, magnitude: f64) -> Self {
        let m = 1.0 - magnitude;
        Self::new(1.0 - (1.0 - self.red) * m, 1.0 - (1.0 - self.green) * m, 1.0 - (1.0 - self.blue) * m, self.alpha)
    }

    /// Moves each channel toward 0 by `magnitude`.
    pub fn darken(&self, magnitude: f64) -> Self {
        let m = 1.0 - magnitude;
        Self::new(self.red * m, self.green * m, self.blue * m, self.alpha)
    }

    pub fn with_alpha(&self, alpha: f64) -> Self {
        Self::from_alpha(self, alpha)
    }

    pub fn add(&self, r: &Self) -> Self {
        Self::new(self.red + r.red, self.green + r.green, self.blue + r.blue, self.alpha + r.alpha)
    }

    pub fn subtract(&self, r: &Self) -> Self {
        Self::new(self.red - r.red, self.green - r.green, self.blue - r.blue, self.alpha - r.alpha)
    }

    pub fn multiply(&self, r: &Self) -> Self {
        Self::new(self.red * r.red, self.green * r.green, self.blue * r.blue, self.alpha * r.alpha)
    }

    pub fn divide(&self, r: &Self) -> Self {
        Self::new(self.red / r.red, self.green / r.green, self.blue / r.blue, self.alpha / r.alpha)
    }

    /// Component-wise remainder with the dividend's sign, like JS `%`.
    pub fn modulo(&self, r: &Self) -> Self {
        Self::new(self.red % r.red, self.green % r.green, self.blue % r.blue, self.alpha % r.alpha)
    }

    pub fn lerp(&self, end: &Self, t: f64) -> Self {
        Self::new(
            lerp(self.red, end.red, t),
            lerp(self.green, end.green, t),
            lerp(self.blue, end.blue, t),
            lerp(self.alpha, end.alpha, t),
        )
    }

    pub fn multiply_by_scalar(&self, s: f64) -> Self {
        Self::new(self.red * s, self.green * s, self.blue * s, self.alpha * s)
    }

    pub fn divide_by_scalar(&self, s: f64) -> Self {
        Self::new(self.red / s, self.green / s, self.blue / s, self.alpha / s)
    }

    /// A CSS named color by upper-case name, including the `GREY` spellings.
    pub fn named(upper_name: &str) -> Option<Self> {
        NAMED_COLORS.iter().find(|(n, _)| *n == upper_name).map(|(_, c)| *c)
    }

    pub const TRANSPARENT: Self = Self::new(0.0, 0.0, 0.0, 0.0);
    pub const ALICEBLUE: Self = hex(0xF0F8FF);
    pub const ANTIQUEWHITE: Self = hex(0xFAEBD7);
    pub const AQUA: Self = hex(0x00FFFF);
    pub const AQUAMARINE: Self = hex(0x7FFFD4);
    pub const AZURE: Self = hex(0xF0FFFF);
    pub const BEIGE: Self = hex(0xF5F5DC);
    pub const BISQUE: Self = hex(0xFFE4C4);
    pub const BLACK: Self = hex(0x000000);
    pub const BLANCHEDALMOND: Self = hex(0xFFEBCD);
    pub const BLUE: Self = hex(0x0000FF);
    pub const BLUEVIOLET: Self = hex(0x8A2BE2);
    pub const BROWN: Self = hex(0xA52A2A);
    pub const BURLYWOOD: Self = hex(0xDEB887);
    pub const CADETBLUE: Self = hex(0x5F9EA0);
    pub const CHARTREUSE: Self = hex(0x7FFF00);
    pub const CHOCOLATE: Self = hex(0xD2691E);
    pub const CORAL: Self = hex(0xFF7F50);
    pub const CORNFLOWERBLUE: Self = hex(0x6495ED);
    pub const CORNSILK: Self = hex(0xFFF8DC);
    pub const CRIMSON: Self = hex(0xDC143C);
    pub const CYAN: Self = hex(0x00FFFF);
    pub const DARKBLUE: Self = hex(0x00008B);
    pub const DARKCYAN: Self = hex(0x008B8B);
    pub const DARKGOLDENROD: Self = hex(0xB8860B);
    pub const DARKGRAY: Self = hex(0xA9A9A9);
    pub const DARKGREEN: Self = hex(0x006400);
    pub const DARKKHAKI: Self = hex(0xBDB76B);
    pub const DARKMAGENTA: Self = hex(0x8B008B);
    pub const DARKOLIVEGREEN: Self = hex(0x556B2F);
    pub const DARKORANGE: Self = hex(0xFF8C00);
    pub const DARKORCHID: Self = hex(0x9932CC);
    pub const DARKRED: Self = hex(0x8B0000);
    pub const DARKSALMON: Self = hex(0xE9967A);
    pub const DARKSEAGREEN: Self = hex(0x8FBC8F);
    pub const DARKSLATEBLUE: Self = hex(0x483D8B);
    pub const DARKSLATEGRAY: Self = hex(0x2F4F4F);
    pub const DARKTURQUOISE: Self = hex(0x00CED1);
    pub const DARKVIOLET: Self = hex(0x9400D3);
    pub const DEEPPINK: Self = hex(0xFF1493);
    pub const DEEPSKYBLUE: Self = hex(0x00BFFF);
    pub const DIMGRAY: Self = hex(0x696969);
    pub const DODGERBLUE: Self = hex(0x1E90FF);
    pub const FIREBRICK: Self = hex(0xB22222);
    pub const FLORALWHITE: Self = hex(0xFFFAF0);
    pub const FORESTGREEN: Self = hex(0x228B22);
    pub const FUCHSIA: Self = hex(0xFF00FF);
    pub const GAINSBORO: Self = hex(0xDCDCDC);
    pub const GHOSTWHITE: Self = hex(0xF8F8FF);
    pub const GOLD: Self = hex(0xFFD700);
    pub const GOLDENROD: Self = hex(0xDAA520);
    pub const GRAY: Self = hex(0x808080);
    pub const GREEN: Self = hex(0x008000);
    pub const GREENYELLOW: Self = hex(0xADFF2F);
    pub const HONEYDEW: Self = hex(0xF0FFF0);
    pub const HOTPINK: Self = hex(0xFF69B4);
    pub const INDIANRED: Self = hex(0xCD5C5C);
    pub const INDIGO: Self = hex(0x4B0082);
    pub const IVORY: Self = hex(0xFFFFF0);
    pub const KHAKI: Self = hex(0xF0E68C);
    pub const LAVENDER: Self = hex(0xE6E6FA);
    pub const LAVENDAR_BLUSH: Self = hex(0xFFF0F5);
    pub const LAWNGREEN: Self = hex(0x7CFC00);
    pub const LEMONCHIFFON: Self = hex(0xFFFACD);
    pub const LIGHTBLUE: Self = hex(0xADD8E6);
    pub const LIGHTCORAL: Self = hex(0xF08080);
    pub const LIGHTCYAN: Self = hex(0xE0FFFF);
    pub const LIGHTGOLDENRODYELLOW: Self = hex(0xFAFAD2);
    pub const LIGHTGRAY: Self = hex(0xD3D3D3);
    pub const LIGHTGREEN: Self = hex(0x90EE90);
    pub const LIGHTPINK: Self = hex(0xFFB6C1);
    pub const LIGHTSEAGREEN: Self = hex(0x20B2AA);
    pub const LIGHTSKYBLUE: Self = hex(0x87CEFA);
    pub const LIGHTSLATEGRAY: Self = hex(0x778899);
    pub const LIGHTSTEELBLUE: Self = hex(0xB0C4DE);
    pub const LIGHTYELLOW: Self = hex(0xFFFFE0);
    pub const LIME: Self = hex(0x00FF00);
    pub const LIMEGREEN: Self = hex(0x32CD32);
    pub const LINEN: Self = hex(0xFAF0E6);
    pub const MAGENTA: Self = hex(0xFF00FF);
    pub const MAROON: Self = hex(0x800000);
    pub const MEDIUMAQUAMARINE: Self = hex(0x66CDAA);
    pub const MEDIUMBLUE: Self = hex(0x0000CD);
    pub const MEDIUMORCHID: Self = hex(0xBA55D3);
    pub const MEDIUMPURPLE: Self = hex(0x9370DB);
    pub const MEDIUMSEAGREEN: Self = hex(0x3CB371);
    pub const MEDIUMSLATEBLUE: Self = hex(0x7B68EE);
    pub const MEDIUMSPRINGGREEN: Self = hex(0x00FA9A);
    pub const MEDIUMTURQUOISE: Self = hex(0x48D1CC);
    pub const MEDIUMVIOLETRED: Self = hex(0xC71585);
    pub const MIDNIGHTBLUE: Self = hex(0x191970);
    pub const MINTCREAM: Self = hex(0xF5FFFA);
    pub const MISTYROSE: Self = hex(0xFFE4E1);
    pub const MOCCASIN: Self = hex(0xFFE4B5);
    pub const NAVAJOWHITE: Self = hex(0xFFDEAD);
    pub const NAVY: Self = hex(0x000080);
    pub const OLDLACE: Self = hex(0xFDF5E6);
    pub const OLIVE: Self = hex(0x808000);
    pub const OLIVEDRAB: Self = hex(0x6B8E23);
    pub const ORANGE: Self = hex(0xFFA500);
    pub const ORANGERED: Self = hex(0xFF4500);
    pub const ORCHID: Self = hex(0xDA70D6);
    pub const PALEGOLDENROD: Self = hex(0xEEE8AA);
    pub const PALEGREEN: Self = hex(0x98FB98);
    pub const PALETURQUOISE: Self = hex(0xAFEEEE);
    pub const PALEVIOLETRED: Self = hex(0xDB7093);
    pub const PAPAYAWHIP: Self = hex(0xFFEFD5);
    pub const PEACHPUFF: Self = hex(0xFFDAB9);
    pub const PERU: Self = hex(0xCD853F);
    pub const PINK: Self = hex(0xFFC0CB);
    pub const PLUM: Self = hex(0xDDA0DD);
    pub const POWDERBLUE: Self = hex(0xB0E0E6);
    pub const PURPLE: Self = hex(0x800080);
    pub const RED: Self = hex(0xFF0000);
    pub const ROSYBROWN: Self = hex(0xBC8F8F);
    pub const ROYALBLUE: Self = hex(0x4169E1);
    pub const SADDLEBROWN: Self = hex(0x8B4513);
    pub const SALMON: Self = hex(0xFA8072);
    pub const SANDYBROWN: Self = hex(0xF4A460);
    pub const SEAGREEN: Self = hex(0x2E8B57);
    pub const SEASHELL: Self = hex(0xFFF5EE);
    pub const SIENNA: Self = hex(0xA0522D);
    pub const SILVER: Self = hex(0xC0C0C0);
    pub const SKYBLUE: Self = hex(0x87CEEB);
    pub const SLATEBLUE: Self = hex(0x6A5ACD);
    pub const SLATEGRAY: Self = hex(0x708090);
    pub const SNOW: Self = hex(0xFFFAFA);
    pub const SPRINGGREEN: Self = hex(0x00FF7F);
    pub const STEELBLUE: Self = hex(0x4682B4);
    pub const TAN: Self = hex(0xD2B48C);
    pub const TEAL: Self = hex(0x008080);
    pub const THISTLE: Self = hex(0xD8BFD8);
    pub const TOMATO: Self = hex(0xFF6347);
    pub const TURQUOISE: Self = hex(0x40E0D0);
    pub const VIOLET: Self = hex(0xEE82EE);
    pub const WHEAT: Self = hex(0xF5DEB3);
    pub const WHITE: Self = hex(0xFFFFFF);
    pub const WHITESMOKE: Self = hex(0xF5F5F5);
    pub const YELLOW: Self = hex(0xFFFF00);
    pub const YELLOWGREEN: Self = hex(0x9ACD32);
    pub const DARKGREY: Self = Self::DARKGRAY;
    pub const DARKSLATEGREY: Self = Self::DARKSLATEGRAY;
    pub const DIMGREY: Self = Self::DIMGRAY;
    pub const GREY: Self = Self::GRAY;
    pub const LIGHTGREY: Self = Self::LIGHTGRAY;
    pub const LIGHTSLATEGREY: Self = Self::LIGHTSLATEGRAY;
    pub const SLATEGREY: Self = Self::SLATEGRAY;
}

/// Every named color Cesium defines, by its upper-case property name.
pub const NAMED_COLORS: &[(&str, Color)] = &[
    ("ALICEBLUE", Color::ALICEBLUE),
    ("ANTIQUEWHITE", Color::ANTIQUEWHITE),
    ("AQUA", Color::AQUA),
    ("AQUAMARINE", Color::AQUAMARINE),
    ("AZURE", Color::AZURE),
    ("BEIGE", Color::BEIGE),
    ("BISQUE", Color::BISQUE),
    ("BLACK", Color::BLACK),
    ("BLANCHEDALMOND", Color::BLANCHEDALMOND),
    ("BLUE", Color::BLUE),
    ("BLUEVIOLET", Color::BLUEVIOLET),
    ("BROWN", Color::BROWN),
    ("BURLYWOOD", Color::BURLYWOOD),
    ("CADETBLUE", Color::CADETBLUE),
    ("CHARTREUSE", Color::CHARTREUSE),
    ("CHOCOLATE", Color::CHOCOLATE),
    ("CORAL", Color::CORAL),
    ("CORNFLOWERBLUE", Color::CORNFLOWERBLUE),
    ("CORNSILK", Color::CORNSILK),
    ("CRIMSON", Color::CRIMSON),
    ("CYAN", Color::CYAN),
    ("DARKBLUE", Color::DARKBLUE),
    ("DARKCYAN", Color::DARKCYAN),
    ("DARKGOLDENROD", Color::DARKGOLDENROD),
    ("DARKGRAY", Color::DARKGRAY),
    ("DARKGREEN", Color::DARKGREEN),
    ("DARKKHAKI", Color::DARKKHAKI),
    ("DARKMAGENTA", Color::DARKMAGENTA),
    ("DARKOLIVEGREEN", Color::DARKOLIVEGREEN),
    ("DARKORANGE", Color::DARKORANGE),
    ("DARKORCHID", Color::DARKORCHID),
    ("DARKRED", Color::DARKRED),
    ("DARKSALMON", Color::DARKSALMON),
    ("DARKSEAGREEN", Color::DARKSEAGREEN),
    ("DARKSLATEBLUE", Color::DARKSLATEBLUE),
    ("DARKSLATEGRAY", Color::DARKSLATEGRAY),
    ("DARKTURQUOISE", Color::DARKTURQUOISE),
    ("DARKVIOLET", Color::DARKVIOLET),
    ("DEEPPINK", Color::DEEPPINK),
    ("DEEPSKYBLUE", Color::DEEPSKYBLUE),
    ("DIMGRAY", Color::DIMGRAY),
    ("DODGERBLUE", Color::DODGERBLUE),
    ("FIREBRICK", Color::FIREBRICK),
    ("FLORALWHITE", Color::FLORALWHITE),
    ("FORESTGREEN", Color::FORESTGREEN),
    ("FUCHSIA", Color::FUCHSIA),
    ("GAINSBORO", Color::GAINSBORO),
    ("GHOSTWHITE", Color::GHOSTWHITE),
    ("GOLD", Color::GOLD),
    ("GOLDENROD", Color::GOLDENROD),
    ("GRAY", Color::GRAY),
    ("GREEN", Color::GREEN),
    ("GREENYELLOW", Color::GREENYELLOW),
    ("HONEYDEW", Color::HONEYDEW),
    ("HOTPINK", Color::HOTPINK),
    ("INDIANRED", Color::INDIANRED),
    ("INDIGO", Color::INDIGO),
    ("IVORY", Color::IVORY),
    ("KHAKI", Color::KHAKI),
    ("LAVENDER", Color::LAVENDER),
    ("LAVENDAR_BLUSH", Color::LAVENDAR_BLUSH),
    ("LAWNGREEN", Color::LAWNGREEN),
    ("LEMONCHIFFON", Color::LEMONCHIFFON),
    ("LIGHTBLUE", Color::LIGHTBLUE),
    ("LIGHTCORAL", Color::LIGHTCORAL),
    ("LIGHTCYAN", Color::LIGHTCYAN),
    ("LIGHTGOLDENRODYELLOW", Color::LIGHTGOLDENRODYELLOW),
    ("LIGHTGRAY", Color::LIGHTGRAY),
    ("LIGHTGREEN", Color::LIGHTGREEN),
    ("LIGHTPINK", Color::LIGHTPINK),
    ("LIGHTSEAGREEN", Color::LIGHTSEAGREEN),
    ("LIGHTSKYBLUE", Color::LIGHTSKYBLUE),
    ("LIGHTSLATEGRAY", Color::LIGHTSLATEGRAY),
    ("LIGHTSTEELBLUE", Color::LIGHTSTEELBLUE),
    ("LIGHTYELLOW", Color::LIGHTYELLOW),
    ("LIME", Color::LIME),
    ("LIMEGREEN", Color::LIMEGREEN),
    ("LINEN", Color::LINEN),
    ("MAGENTA", Color::MAGENTA),
    ("MAROON", Color::MAROON),
    ("MEDIUMAQUAMARINE", Color::MEDIUMAQUAMARINE),
    ("MEDIUMBLUE", Color::MEDIUMBLUE),
    ("MEDIUMORCHID", Color::MEDIUMORCHID),
    ("MEDIUMPURPLE", Color::MEDIUMPURPLE),
    ("MEDIUMSEAGREEN", Color::MEDIUMSEAGREEN),
    ("MEDIUMSLATEBLUE", Color::MEDIUMSLATEBLUE),
    ("MEDIUMSPRINGGREEN", Color::MEDIUMSPRINGGREEN),
    ("MEDIUMTURQUOISE", Color::MEDIUMTURQUOISE),
    ("MEDIUMVIOLETRED", Color::MEDIUMVIOLETRED),
    ("MIDNIGHTBLUE", Color::MIDNIGHTBLUE),
    ("MINTCREAM", Color::MINTCREAM),
    ("MISTYROSE", Color::MISTYROSE),
    ("MOCCASIN", Color::MOCCASIN),
    ("NAVAJOWHITE", Color::NAVAJOWHITE),
    ("NAVY", Color::NAVY),
    ("OLDLACE", Color::OLDLACE),
    ("OLIVE", Color::OLIVE),
    ("OLIVEDRAB", Color::OLIVEDRAB),
    ("ORANGE", Color::ORANGE),
    ("ORANGERED", Color::ORANGERED),
    ("ORCHID", Color::ORCHID),
    ("PALEGOLDENROD", Color::PALEGOLDENROD),
    ("PALEGREEN", Color::PALEGREEN),
    ("PALETURQUOISE", Color::PALETURQUOISE),
    ("PALEVIOLETRED", Color::PALEVIOLETRED),
    ("PAPAYAWHIP", Color::PAPAYAWHIP),
    ("PEACHPUFF", Color::PEACHPUFF),
    ("PERU", Color::PERU),
    ("PINK", Color::PINK),
    ("PLUM", Color::PLUM),
    ("POWDERBLUE", Color::POWDERBLUE),
    ("PURPLE", Color::PURPLE),
    ("RED", Color::RED),
    ("ROSYBROWN", Color::ROSYBROWN),
    ("ROYALBLUE", Color::ROYALBLUE),
    ("SADDLEBROWN", Color::SADDLEBROWN),
    ("SALMON", Color::SALMON),
    ("SANDYBROWN", Color::SANDYBROWN),
    ("SEAGREEN", Color::SEAGREEN),
    ("SEASHELL", Color::SEASHELL),
    ("SIENNA", Color::SIENNA),
    ("SILVER", Color::SILVER),
    ("SKYBLUE", Color::SKYBLUE),
    ("SLATEBLUE", Color::SLATEBLUE),
    ("SLATEGRAY", Color::SLATEGRAY),
    ("SNOW", Color::SNOW),
    ("SPRINGGREEN", Color::SPRINGGREEN),
    ("STEELBLUE", Color::STEELBLUE),
    ("TAN", Color::TAN),
    ("TEAL", Color::TEAL),
    ("THISTLE", Color::THISTLE),
    ("TOMATO", Color::TOMATO),
    ("TURQUOISE", Color::TURQUOISE),
    ("VIOLET", Color::VIOLET),
    ("WHEAT", Color::WHEAT),
    ("WHITE", Color::WHITE),
    ("WHITESMOKE", Color::WHITESMOKE),
    ("YELLOW", Color::YELLOW),
    ("YELLOWGREEN", Color::YELLOWGREEN),
    ("DARKGREY", Color::DARKGREY),
    ("DARKSLATEGREY", Color::DARKSLATEGREY),
    ("DIMGREY", Color::DIMGREY),
    ("GREY", Color::GREY),
    ("LIGHTGREY", Color::LIGHTGREY),
    ("LIGHTSLATEGREY", Color::LIGHTSLATEGREY),
    ("SLATEGREY", Color::SLATEGREY),
    ("TRANSPARENT", Color::TRANSPARENT),
];

/// `#rgb`, `#rgba`, `#rrggbb`, or `#rrggbbaa`, case-insensitive.
fn parse_hex(s: &str) -> Option<Color> {
    let digits = s.strip_prefix('#')?;
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = |t: &str| u32::from_str_radix(t, 16).ok().map(f64::from);
    match digits.len() {
        3 | 4 => {
            let d: Vec<&str> = (0..digits.len()).map(|i| &digits[i..i + 1]).collect();
            Some(Color::new(
                v(d[0])? / 15.0,
                v(d[1])? / 15.0,
                v(d[2])? / 15.0,
                v(d.get(3).copied().unwrap_or("f"))? / 15.0,
            ))
        }
        6 | 8 => Some(Color::new(
            v(&digits[0..2])? / 255.0,
            v(&digits[2..4])? / 255.0,
            v(&digits[4..6])? / 255.0,
            v(digits.get(6..8).unwrap_or("ff"))? / 255.0,
        )),
        _ => None,
    }
}

/// Matches `\s*\( n sep n sep n (sep a)? \s*\)` from Cesium's rgb/hsl
/// regexes, where `n` is `[0-9.]+%?`, `a` is `[0-9.]+`, and the separator
/// is `[,\s]+` (also `/` before the alpha). Returns the three channels and
/// the optional alpha. `percent_any` allows `%` on every channel (rgb);
/// hsl checks its own `%` pattern.
fn parse_parenthesized(rest: &str, percent_any: bool) -> Option<([&str; 3], Option<&str>)> {
    let _ = percent_any;
    let body = rest.trim_start().strip_prefix('(')?.trim_end().strip_suffix(')')?;
    let is_num = |c: char| c.is_ascii_digit() || c == '.';
    let mut tokens: Vec<&str> = Vec::new();
    let mut seps: Vec<&str> = Vec::new();
    let mut i = 0;
    let bytes: Vec<(usize, char)> = body.char_indices().collect();
    // Leading whitespace inside the parenthesis.
    while i < bytes.len() && bytes[i].1.is_whitespace() {
        i += 1;
    }
    while i < bytes.len() {
        let start = bytes[i].0;
        let mut j = i;
        while j < bytes.len() && is_num(bytes[j].1) {
            j += 1;
        }
        if j == i {
            return None;
        }
        if j < bytes.len() && bytes[j].1 == '%' {
            j += 1;
        }
        let end = if j < bytes.len() { bytes[j].0 } else { body.len() };
        tokens.push(&body[start..end]);
        i = j;
        let sep_start = i;
        while i < bytes.len() && (bytes[i].1 == ',' || bytes[i].1 == '/' || bytes[i].1.is_whitespace()) {
            i += 1;
        }
        let sep_end = if i < bytes.len() { bytes[i].0 } else { body.len() };
        let sep = &body[bytes.get(sep_start).map_or(body.len(), |b| b.0)..sep_end];
        if i < bytes.len() {
            if sep.is_empty() {
                return None;
            }
            seps.push(sep);
        } else if sep.chars().any(|c| !c.is_whitespace()) {
            return None;
        }
    }
    if tokens.len() != 3 && tokens.len() != 4 {
        return None;
    }
    // The first two separators may not contain '/', and each must hold a
    // comma or whitespace.
    for sep in &seps[..2] {
        if sep.contains('/') || sep.is_empty() {
            return None;
        }
    }
    if tokens.len() == 4 {
        if tokens[3].ends_with('%') {
            return None;
        }
        Some(([tokens[0], tokens[1], tokens[2]], Some(tokens[3])))
    } else {
        Some(([tokens[0], tokens[1], tokens[2]], None))
    }
}

/// JavaScript `parseFloat` for strings of digits, dots, and an optional `%`:
/// the longest valid decimal prefix, or NaN.
fn js_parse_float(s: &str) -> f64 {
    let mut end = 0;
    let mut seen_dot = false;
    let mut seen_digit = false;
    for (i, c) in s.char_indices() {
        if c.is_ascii_digit() {
            seen_digit = true;
            end = i + 1;
        } else if c == '.' && !seen_dot {
            seen_dot = true;
            if seen_digit {
                end = i + 1;
            }
        } else {
            break;
        }
    }
    if !seen_digit {
        return f64::NAN;
    }
    let t = s[..end].trim_end_matches('.');
    let t = if t.starts_with('.') { format!("0{t}") } else { t.to_string() };
    t.parse().unwrap_or(f64::NAN)
}
