//! The subset of urijs (the URI library CesiumJS bundles) that Cesium's URI
//! helpers use: parsing, path normalization, resolution against a base, and
//! rebuilding. Also JavaScript's `encodeURIComponent`/`decodeURIComponent`.

/// JavaScript's `encodeURIComponent`.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// JavaScript's `decodeURIComponent`; `None` where it throws `URIError`.
pub fn decode_uri_component(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// urijs's `strictEncodeURIComponent`: also escapes `!'()*`.
fn strict_encode(s: &str) -> String {
    encode_uri_component(s)
        .replace('!', "%21")
        .replace('\'', "%27")
        .replace('(', "%28")
        .replace(')', "%29")
        .replace('*', "%2A")
}

/// urijs's `encodePathSegment`: strict encoding with `$&+,;=:@` restored.
fn encode_path_segment(s: &str) -> String {
    let mut e = strict_encode(s);
    for (code, c) in
        [("%24", "$"), ("%26", "&"), ("%2B", "+"), ("%2C", ","), ("%3B", ";"), ("%3D", "="), ("%3A", ":"), ("%40", "@")]
    {
        e = e.replace(code, c);
    }
    e
}

/// urijs's `encodeUrnPathSegment`: strict encoding with RFC 2141's
/// `!$'()*+,;=@` restored.
fn encode_urn_path_segment(s: &str) -> String {
    let mut e = strict_encode(s);
    for (code, c) in [
        ("%21", "!"),
        ("%24", "$"),
        ("%27", "'"),
        ("%28", "("),
        ("%29", ")"),
        ("%2A", "*"),
        ("%2B", "+"),
        ("%2C", ","),
        ("%3B", ";"),
        ("%3D", "="),
        ("%40", "@"),
    ] {
        e = e.replace(code, c);
    }
    e
}

/// urijs's `recodePath`. A segment that does not decode is kept as it is.
fn recode_path(path: &str) -> String {
    path.split('/')
        .map(|seg| decode_uri_component(seg).map_or_else(|| seg.to_string(), |d| encode_path_segment(&d)))
        .collect::<Vec<_>>()
        .join("/")
}

fn is_protocol(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Uri {
    pub protocol: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub hostname: Option<String>,
    pub port: Option<String>,
    pub path: String,
    pub query: Option<String>,
    pub fragment: Option<String>,
    pub urn: bool,
}

fn non_empty(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

impl Uri {
    /// urijs's `URI.parse`.
    pub fn parse(input: &str) -> Self {
        let mut parts = Uri::default();
        let mut s: String = input
            .trim_start_matches(|c: char| {
                c <= '\u{20}'
                    || "\u{a0}\u{1680}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}".contains(c)
                    || ('\u{2000}'..='\u{200a}').contains(&c)
            })
            .chars()
            .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
            .collect();

        if let Some(pos) = s.find('#') {
            parts.fragment = non_empty(&s[pos + 1..]);
            s.truncate(pos);
        }
        if let Some(pos) = s.find('?') {
            parts.query = non_empty(&s[pos + 1..]);
            s.truncate(pos);
        }

        // Slashes after web schemes, and leading slashes, lose their meaning.
        let lower = s.to_ascii_lowercase();
        for scheme in ["https", "http", "ftp", "wss", "ws", ""] {
            if lower.starts_with(scheme) && s[scheme.len()..].starts_with(':') {
                let rest = s[scheme.len()..].trim_start_matches(':').trim_start_matches(['/', '\\']);
                s = format!("{}://{}", &s[..scheme.len()], rest);
                break;
            }
        }
        let leading = s.chars().take_while(|c| *c == '/' || *c == '\\').count();
        if leading >= 2 {
            s = format!("//{}", &s[leading..]);
        }

        if let Some(rest) = s.strip_prefix("//") {
            parts.protocol = None;
            s = parts.parse_authority(rest);
        } else if let Some(pos) = s.find(':') {
            let protocol = &s[..pos];
            if !protocol.is_empty() && !is_protocol(protocol) {
                parts.protocol = None;
            } else {
                parts.protocol = non_empty(protocol);
                if s[pos + 1..].chars().take(2).map(|c| if c == '\\' { '/' } else { c }).collect::<String>() == "//" {
                    let rest = s[pos + 3..].to_string();
                    s = parts.parse_authority(&rest);
                } else {
                    s = s[pos + 1..].to_string();
                    parts.urn = true;
                }
            }
        }
        parts.path = s;
        parts
    }

    fn parse_authority(&mut self, input: &str) -> String {
        // Userinfo.
        let normalized = input.replace('\\', "/");
        let first_slash = normalized.find('/');
        let search_end = first_slash.unwrap_or(normalized.len().saturating_sub(1));
        let at = normalized[..(search_end + 1).min(normalized.len())].rfind('@');
        let mut s = input.to_string();
        match at {
            Some(pos) if first_slash.is_none_or(|f| pos < f) => {
                let info: Vec<&str> = normalized[..pos].split(':').collect();
                self.username = non_empty(info[0]).map(|u| decode_uri_component(&u).unwrap_or(u));
                let password = info[1..].join(":");
                self.password = (info.len() > 1 && !info[1].is_empty())
                    .then(|| decode_uri_component(&password).unwrap_or(password));
                s = input[pos + 1..].to_string();
            }
            _ => {
                self.username = None;
                self.password = None;
            }
        }

        // Host and port.
        let s = s.replace('\\', "/");
        let mut pos = s.find('/').unwrap_or(s.len());
        if s.starts_with('[') {
            let bracket = s.find(']').unwrap_or(s.len());
            self.hostname = non_empty(&s[1..bracket.min(s.len())]);
            self.port = non_empty(s.get(bracket + 2..pos).unwrap_or(""));
            if self.port.as_deref() == Some("/") {
                self.port = None;
            }
        } else {
            let first_colon = s.find(':');
            let first_slash = s.find('/');
            let next_colon = first_colon.and_then(|c| s[c + 1..].find(':').map(|n| n + c + 1));
            if next_colon.is_some_and(|n| first_slash.is_none_or(|f| n < f)) {
                self.hostname = non_empty(&s[..pos]);
                self.port = None;
            } else {
                let mut hp = s[..pos].splitn(3, ':');
                self.hostname = hp.next().and_then(non_empty);
                self.port = hp.next().and_then(non_empty);
            }
        }
        let mut s = s;
        if self.hostname.is_some() && !s[pos..].starts_with('/') {
            pos += 1;
            s = format!("/{s}");
        }
        let rest = &s[pos.min(s.len())..];
        if rest.is_empty() {
            "/".to_string()
        } else {
            rest.to_string()
        }
    }

    /// urijs's `path()`.
    pub fn path(&self) -> String {
        if self.path.is_empty() && self.hostname.is_some() {
            "/".to_string()
        } else {
            self.path.clone()
        }
    }

    pub fn query(&self) -> &str {
        self.query.as_deref().unwrap_or("")
    }

    pub fn fragment(&self) -> &str {
        self.fragment.as_deref().unwrap_or("")
    }

    pub fn scheme(&self) -> &str {
        self.protocol.as_deref().unwrap_or("")
    }

    fn is_relative(&self) -> bool {
        !self.urn && self.hostname.is_none()
    }

    fn filename(&self) -> &str {
        if self.urn || self.path.is_empty() || self.path == "/" {
            return "";
        }
        &self.path[self.path.rfind('/').map_or(0, |p| p + 1)..]
    }

    fn directory(&self) -> String {
        if self.urn || (self.path.is_empty() && self.hostname.is_none()) {
            return String::new();
        }
        if self.path == "/" {
            return "/".to_string();
        }
        let end = self.path.len() as isize - self.filename().len() as isize - 1;
        let res = if end > 0 { &self.path[..end as usize] } else { "" };
        if res.is_empty() && self.hostname.is_some() {
            "/".to_string()
        } else {
            res.to_string()
        }
    }

    /// urijs's `normalizePath`.
    pub fn normalize_path(&mut self) {
        if self.path.is_empty() || self.path == "/" {
            return;
        }
        if self.urn {
            self.path = self
                .path
                .split(':')
                .map(|seg| decode_uri_component(seg).map_or_else(|| seg.to_string(), |d| encode_urn_path_segment(&d)))
                .collect::<Vec<_>>()
                .join(":");
            return;
        }
        let mut path = recode_path(&self.path);
        let was_relative = !path.starts_with('/');
        if was_relative {
            path = format!("/{path}");
        }
        if path.ends_with("/..") || path.ends_with("/.") {
            path.push('/');
        }
        // Resolve "./" runs and a trailing "/.", then collapse slashes.
        loop {
            let replaced = path.replace("/./", "/");
            if replaced == path {
                break;
            }
            path = replaced;
        }
        if path.ends_with("/.") {
            path.truncate(path.len() - 1);
        }
        while path.contains("//") {
            path = path.replace("//", "/");
        }
        let mut leading_parents = String::new();
        if was_relative {
            let mut rest = &path[1..];
            while let Some(r) = rest.strip_prefix("../") {
                leading_parents.push_str("../");
                rest = r;
            }
        }
        loop {
            let parent = path
                .match_indices("/..")
                .map(|(i, _)| i)
                .find(|&i| matches!(path.as_bytes().get(i + 3), None | Some(b'/')));
            match parent {
                None => break,
                Some(0) => path = path[3..].to_string(),
                Some(p) => {
                    let pos = path[..p].rfind('/').unwrap_or(p);
                    path = format!("{}{}", &path[..pos], &path[p + 3..]);
                }
            }
        }
        if was_relative && self.is_relative() {
            path = format!("{leading_parents}{}", &path[1.min(path.len())..]);
        }
        self.path = path;
    }

    /// urijs's `normalize()`, for the parts Cesium reads: protocol and host
    /// case, default ports, and the path.
    pub fn normalize(&mut self) {
        if let Some(p) = &mut self.protocol {
            *p = p.to_ascii_lowercase();
        }
        if let Some(h) = &mut self.hostname {
            *h = h.to_ascii_lowercase();
        }
        let default_port = match self.protocol.as_deref() {
            Some("http") | Some("ws") => Some("80"),
            Some("https") | Some("wss") => Some("443"),
            Some("ftp") => Some("21"),
            _ => None,
        };
        if self.port.is_some() && self.port.as_deref() == default_port {
            self.port = None;
        }
        self.normalize_path();
    }

    /// urijs's `absoluteTo`. `None` for a URN, which urijs refuses.
    pub fn absolute_to(&self, base: &str) -> Option<Uri> {
        if self.urn {
            return None;
        }
        let base = Uri::parse(base);
        let mut resolved = self.clone();
        if resolved.protocol.is_some() {
            return Some(resolved);
        }
        resolved.protocol = base.protocol.clone();
        if self.hostname.is_some() {
            return Some(resolved);
        }
        resolved.username = base.username.clone();
        resolved.password = base.password.clone();
        resolved.hostname = base.hostname.clone();
        resolved.port = base.port.clone();
        if resolved.path.is_empty() {
            resolved.path = base.path.clone();
            if resolved.query.is_none() {
                resolved.query = base.query.clone();
            }
        } else {
            // urijs compares `path.substring(-2)`, which is the whole path.
            if resolved.path == ".." {
                resolved.path.push('/');
            }
            if !resolved.path().starts_with('/') {
                let mut basedir = base.directory();
                if basedir.is_empty() && base.path().starts_with('/') {
                    basedir = "/".to_string();
                }
                resolved.path = if basedir.is_empty() { resolved.path } else { format!("{basedir}/{}", resolved.path) };
                resolved.normalize_path();
            }
        }
        Some(resolved)
    }
}

impl std::fmt::Display for Uri {
    /// urijs's `URI.build`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut t = String::new();
        let mut require_absolute_path = false;
        if let Some(p) = &self.protocol {
            t.push_str(p);
            t.push(':');
        }
        if !self.urn && (!t.is_empty() || self.hostname.is_some()) {
            t.push_str("//");
            require_absolute_path = true;
        }
        let mut userinfo = String::new();
        if let Some(u) = &self.username {
            userinfo.push_str(&strict_encode(u));
        }
        if let Some(p) = &self.password {
            userinfo.push(':');
            userinfo.push_str(&strict_encode(p));
        }
        if !userinfo.is_empty() {
            userinfo.push('@');
        }
        t.push_str(&userinfo);
        if let Some(h) = &self.hostname {
            if h.contains(':') {
                t.push_str(&format!("[{h}]"));
            } else {
                t.push_str(h);
            }
            if let Some(p) = &self.port {
                t.push(':');
                t.push_str(p);
            }
        }
        if !self.path.starts_with('/') && require_absolute_path {
            t.push('/');
        }
        t.push_str(&self.path);
        if let Some(q) = &self.query {
            t.push('?');
            t.push_str(q);
        }
        if let Some(fr) = &self.fragment {
            t.push('#');
            t.push_str(fr);
        }
        f.write_str(&t)
    }
}
