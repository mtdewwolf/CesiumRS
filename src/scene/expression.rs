//! CesiumJS `Expression`: the 3D Tiles Styling language evaluator.
//!
//! Ports the pinned `jsep` grammar used by Cesium and the runtime AST that
//! evaluates and emits GLSL. Values keep JavaScript's `undefined` distinct from
//! `null`, because style expressions rely on that distinction.

use crate::core::math::{clamp, js_number_to_string, js_round, js_to_fixed, lerp, log2, sign, to_degrees, to_radians};
use crate::core::{Cartesian2, Cartesian3, Cartesian4, Color, RuntimeError};
use regex::RegexBuilder;
use serde_json::Value as Json;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

/// A feature whose properties participate in a style expression.
///
/// Cesium's `Cesium3DTileFeature` supplies the concrete implementation. `None`
/// properties are `undefined`, not `null`.
pub trait StyleFeature {
    fn property_inherited(&self, _name: &str) -> Option<StyleValue> {
        None
    }
    fn is_exact_class(&self, _class_name: &str) -> bool {
        false
    }
    fn is_class(&self, _class_name: &str) -> bool {
        false
    }
    fn exact_class_name(&self) -> Option<String> {
        None
    }
    /// `feature.content.tileset.timeSinceLoad`, used by `tiles3d_tileset_time`.
    fn tileset_time(&self) -> f64 {
        0.0
    }
}

/// A JavaScript value produced or consumed by a style expression.
#[derive(Clone)]
pub enum StyleValue {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(Vec<StyleValue>),
    Cartesian2(Cartesian2),
    Cartesian3(Cartesian3),
    Cartesian4(Cartesian4),
    /// A JavaScript object; JSON metadata values decode into this variant.
    Object(BTreeMap<String, StyleValue>),
    RegExp(Rc<RefCell<RegExpValue>>),
}

impl fmt::Debug for StyleValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.js_string())
    }
}

impl StyleValue {
    pub fn from_json(value: &Json) -> Self {
        match value {
            Json::Null => Self::Null,
            Json::Bool(b) => Self::Boolean(*b),
            Json::Number(n) => Self::Number(n.as_f64().unwrap_or(f64::NAN)),
            Json::String(s) => Self::String(s.clone()),
            Json::Array(a) => Self::Array(a.iter().map(Self::from_json).collect()),
            Json::Object(o) => Self::Object(o.iter().map(|(k, v)| (k.clone(), Self::from_json(v))).collect()),
        }
    }

    /// JavaScript `String(value)`, including Cesium's vector `toString`.
    pub fn js_string(&self) -> String {
        match self {
            Self::Undefined => "undefined".into(),
            Self::Null => "null".into(),
            Self::Boolean(b) => b.to_string(),
            Self::Number(n) => js_number_to_string(*n),
            Self::String(s) => s.clone(),
            Self::Array(a) => a.iter().map(Self::js_string).collect::<Vec<_>>().join(","),
            Self::Cartesian2(v) => {
                format!("({}, {})", js_number_to_string(v.x), js_number_to_string(v.y))
            }
            Self::Cartesian3(v) => {
                format!("({}, {}, {})", js_number_to_string(v.x), js_number_to_string(v.y), js_number_to_string(v.z))
            }
            Self::Cartesian4(v) => {
                format!(
                    "({}, {}, {}, {})",
                    js_number_to_string(v.x),
                    js_number_to_string(v.y),
                    js_number_to_string(v.z),
                    js_number_to_string(v.w)
                )
            }
            Self::Object(_) => "[object Object]".into(),
            Self::RegExp(r) => r.borrow().to_string(),
        }
    }

    /// JavaScript `Number(value)`.
    pub fn to_number(&self) -> f64 {
        match self {
            Self::Undefined => f64::NAN,
            Self::Null => 0.0,
            Self::Boolean(b) => i32::from(*b) as f64,
            Self::Number(n) => *n,
            Self::String(s) => js_parse_number(s),
            Self::Array(a) => {
                if a.is_empty() {
                    0.0
                } else {
                    js_parse_number(&self.js_string())
                }
            }
            Self::Cartesian2(_) | Self::Cartesian3(_) | Self::Cartesian4(_) | Self::Object(_) => f64::NAN,
            Self::RegExp(_) => f64::NAN,
        }
    }

    /// JavaScript `Boolean(value)`.
    pub fn to_boolean(&self) -> bool {
        match self {
            Self::Undefined | Self::Null => false,
            Self::Boolean(b) => *b,
            Self::Number(n) => *n != 0.0 && !n.is_nan(),
            Self::String(s) => !s.is_empty(),
            _ => true,
        }
    }

    fn is_vector(&self) -> bool {
        matches!(self, Self::Cartesian2(_) | Self::Cartesian3(_) | Self::Cartesian4(_))
    }

    fn vector_component(&self, name: &str) -> Option<StyleValue> {
        let component = match self {
            Self::Cartesian2(v) => match name {
                "x" | "r" | "0" => Some(v.x),
                "y" | "g" | "1" => Some(v.y),
                _ => None,
            },
            Self::Cartesian3(v) => match name {
                "x" | "r" | "0" => Some(v.x),
                "y" | "g" | "1" => Some(v.y),
                "z" | "b" | "2" => Some(v.z),
                _ => None,
            },
            Self::Cartesian4(v) => match name {
                "x" | "r" | "0" => Some(v.x),
                "y" | "g" | "1" => Some(v.y),
                "z" | "b" | "2" => Some(v.z),
                "w" | "a" | "3" => Some(v.w),
                _ => None,
            },
            _ => None,
        };
        component.map(Self::Number)
    }

    fn member(&self, key: &StyleValue) -> StyleValue {
        let key_string = key.js_string();
        if self.is_vector() {
            // Vector components may be accessed with r/g/b/a, x/y/z/w and
            // bracketed indices. Out-of-range components are undefined.
            if let Some(v) = self.vector_component(&key_string) {
                return v;
            }
            return Self::Undefined;
        }
        match self {
            Self::Object(o) => o.get(&key_string).cloned().unwrap_or(Self::Undefined),
            Self::Array(a) => {
                if key_string == "length" {
                    return Self::Number(a.len() as f64);
                }
                js_array_index(&key_string).and_then(|i| a.get(i).cloned()).unwrap_or(Self::Undefined)
            }
            Self::String(s) => {
                if key_string == "length" {
                    return Self::Number(s.chars().count() as f64);
                }
                js_array_index(&key_string)
                    .and_then(|i| s.chars().nth(i))
                    .map(|c| Self::String(c.to_string()))
                    .unwrap_or(Self::Undefined)
            }
            Self::RegExp(r) => match key_string.as_str() {
                "source" => Self::String(r.borrow().pattern.clone()),
                "flags" => Self::String(r.borrow().flags.clone()),
                "global" => Self::Boolean(r.borrow().flags.contains('g')),
                "ignoreCase" => Self::Boolean(r.borrow().flags.contains('i')),
                "multiline" => Self::Boolean(r.borrow().flags.contains('m')),
                "sticky" => Self::Boolean(r.borrow().flags.contains('y')),
                _ => Self::Undefined,
            },
            _ => Self::Undefined,
        }
    }

    fn strict_equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Undefined, Self::Undefined) | (Self::Null, Self::Null) => true,
            (Self::Boolean(a), Self::Boolean(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Cartesian2(a), Self::Cartesian2(b)) => a == b,
            (Self::Cartesian3(a), Self::Cartesian3(b)) => a == b,
            (Self::Cartesian4(a), Self::Cartesian4(b)) => a == b,
            (Self::RegExp(a), Self::RegExp(b)) => Rc::ptr_eq(a, b),
            (Self::Object(_), Self::Object(_)) | (Self::Array(_), Self::Array(_)) => false,
            _ => false,
        }
    }
}

fn js_array_index(key: &str) -> Option<usize> {
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    key.parse().ok()
}

/// JavaScript `Number(string)`: empty is zero, `0x` is hex, otherwise decimal
/// with an optional sign, fraction and exponent.
pub fn js_parse_number(text: &str) -> f64 {
    let text = text.trim();
    if text.is_empty() {
        return 0.0;
    }
    let (sign, rest) = match text.as_bytes()[0] {
        b'+' => (1.0, &text[1..]),
        b'-' => (-1.0, &text[1..]),
        _ => (1.0, text),
    };
    if rest == "Infinity" {
        return sign * f64::INFINITY;
    }
    if let Some(hex) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
        return match u64::from_str_radix(hex, 16) {
            Ok(v) => sign * v as f64,
            Err(_) => f64::NAN,
        };
    }
    if rest.starts_with('.') || rest.starts_with(|c: char| c.is_ascii_digit()) {
        if let Ok(v) = rest.parse::<f64>() {
            return sign * v;
        }
    }
    f64::NAN
}

#[derive(Clone, Debug)]
pub struct RegExpValue {
    pub pattern: String,
    pub flags: String,
    pub last_index: usize,
}

impl RegExpValue {
    pub fn new(pattern: impl Into<String>, flags: impl Into<String>) -> Result<Self, RuntimeError> {
        let pattern = pattern.into();
        let flags = flags.into();
        let mut builder = RegexBuilder::new(&pattern);
        for flag in flags.chars() {
            match flag {
                'g' | 'y' | 'u' | 'd' | 'v' => {}
                'i' => {
                    builder.case_insensitive(true);
                }
                'm' => {
                    builder.multi_line(true);
                }
                's' => {
                    builder.dot_matches_new_line(true);
                }
                other => {
                    return Err(RuntimeError::new(format!("Invalid regular expression flags: {other}")));
                }
            }
        }
        builder.build().map_err(|e| RuntimeError::new(format!("Invalid regular expression: {e}")))?;
        Ok(Self { pattern, flags, last_index: 0 })
    }

    fn compile(&self) -> Result<regex::Regex, RuntimeError> {
        let mut builder = RegexBuilder::new(&self.pattern);
        for flag in self.flags.chars() {
            match flag {
                'i' => {
                    builder.case_insensitive(true);
                }
                'm' => {
                    builder.multi_line(true);
                }
                's' => {
                    builder.dot_matches_new_line(true);
                }
                _ => {}
            }
        }
        builder.build().map_err(|e| RuntimeError::new(format!("Invalid regular expression: {e}")))
    }

    /// `RegExp.prototype.exec`, returning the first capture group.
    pub fn exec_group1(&mut self, input: &str) -> Result<StyleValue, RuntimeError> {
        let re = self.compile()?;
        let global = self.flags.contains('g') || self.flags.contains('y');
        let start = if global { self.last_index.min(input.len()) } else { 0 };
        let found = re.captures_at(input, start);
        match found {
            Some(captures) => {
                if global {
                    self.last_index = captures.get(0).map_or(0, |m| m.end());
                }
                Ok(captures.get(1).map(|m| StyleValue::String(m.as_str().to_string())).unwrap_or(StyleValue::Undefined))
            }
            None => {
                if global {
                    self.last_index = 0;
                }
                Ok(StyleValue::Null)
            }
        }
    }

    /// `RegExp.prototype.test`.
    pub fn test(&mut self, input: &str) -> Result<bool, RuntimeError> {
        let re = self.compile()?;
        let global = self.flags.contains('g') || self.flags.contains('y');
        let start = if global { self.last_index.min(input.len()) } else { 0 };
        let found = if self.flags.contains('y') {
            re.find_at(input, start).is_some_and(|m| m.start() == start)
        } else {
            re.find_at(input, start).is_some()
        };
        if global {
            self.last_index = if found { re.find_at(input, start).map_or(0, |m| m.end()) } else { 0 };
        }
        Ok(found)
    }
}

impl fmt::Display for RegExpValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.flags.is_empty() {
            write!(f, "/{}/", self.pattern)
        } else {
            write!(f, "/{}/{}", self.pattern, self.flags)
        }
    }
}

// ---------------------------------------------------------------------------
// jsep-compatible parser
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f64),
    String(String),
    Ident(String),
    Op(String),
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Question,
    Colon,
    Dot,
}

#[derive(Clone, Debug)]
enum Ast {
    Literal(Literal),
    Identifier(String),
    Unary { op: String, argument: Box<Ast> },
    Binary { op: String, left: Box<Ast>, right: Box<Ast> },
    Conditional { test: Box<Ast>, consequent: Box<Ast>, alternate: Box<Ast> },
    Member { computed: bool, object: Box<Ast>, property: Box<Ast> },
    Call { callee: Box<Ast>, arguments: Vec<Ast> },
    Array(Vec<Ast>),
    Compound,
}

#[derive(Clone, Debug)]
enum Literal {
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    fn new(expression: &str) -> Result<Self, RuntimeError> {
        Ok(Self { tokens: tokenize(expression)?, index: 0 })
    }

    fn parse(mut self) -> Result<Ast, RuntimeError> {
        if self.tokens.is_empty() {
            return Ok(Ast::Compound);
        }
        let ast = self.parse_expression()?;
        if self.index < self.tokens.len() {
            return Err(RuntimeError::new(format!("Unexpected {:?}.", self.tokens[self.index])));
        }
        Ok(ast)
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.index)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.index).cloned();
        if token.is_some() {
            self.index += 1;
        }
        token
    }

    fn parse_expression(&mut self) -> Result<Ast, RuntimeError> {
        let node = self.parse_binary()?;
        if self.peek() == Some(&Token::Question) {
            self.index += 1;
            let consequent = self.parse_expression()?;
            if self.next() != Some(Token::Colon) {
                return Err(RuntimeError::new("Expected :"));
            }
            let alternate = self.parse_expression()?;
            return Ok(Ast::Conditional {
                test: Box::new(node),
                consequent: Box::new(consequent),
                alternate: Box::new(alternate),
            });
        }
        Ok(node)
    }

    fn parse_binary(&mut self) -> Result<Ast, RuntimeError> {
        let left = self.parse_token()?;
        let Some(biop) = self.gobble_binary_op() else {
            return Ok(left);
        };
        let precedence = binary_precedence(&biop);
        let right = self.parse_token()?;
        let mut operands = vec![left, right];
        let mut operators = vec![(biop, precedence)];
        while let Some(next_op) = self.gobble_binary_op() {
            let precedence = binary_precedence(&next_op);
            if precedence == 0 {
                // jsep treats a zero-precedence operator as the end of the
                // expression unless it is the first one. Cesium registers
                // =~ and !~ at zero, so this quirk is observable.
                self.index -= 1;
                break;
            }
            while let Some((previous_op, previous_precedence)) = operators.last() {
                let reduce = if next_op == "**" && previous_op == "**" {
                    precedence > *previous_precedence
                } else {
                    precedence <= *previous_precedence
                };
                if !reduce {
                    break;
                }
                reduce_binary(&mut operands, &mut operators);
            }
            operators.push((next_op, precedence));
            operands.push(self.parse_token()?);
        }
        while !operators.is_empty() {
            reduce_binary(&mut operands, &mut operators);
        }
        Ok(operands.pop().expect("one expression"))
    }

    fn gobble_binary_op(&mut self) -> Option<String> {
        let token = self.peek()?.clone();
        let Token::Op(op) = token else { return None };
        self.index += 1;
        Some(op)
    }

    fn parse_token(&mut self) -> Result<Ast, RuntimeError> {
        let token = self.next().ok_or_else(|| RuntimeError::new("Expected an expression."))?;
        let mut node = match token {
            Token::Number(n) => Ast::Literal(Literal::Number(n)),
            Token::String(s) => Ast::Literal(Literal::String(s)),
            Token::Ident(name) => match name.as_str() {
                "true" => Ast::Literal(Literal::Boolean(true)),
                "false" => Ast::Literal(Literal::Boolean(false)),
                "null" => Ast::Literal(Literal::Null),
                "this" => return Err(RuntimeError::new("Cannot parse expression.")),
                _ => Ast::Identifier(name),
            },
            Token::LParen => {
                let node = self.parse_expression()?;
                if self.next() != Some(Token::RParen) {
                    return Err(RuntimeError::new("Unclosed ("));
                }
                node
            }
            Token::LBracket => {
                let mut elements = Vec::new();
                if self.peek() != Some(&Token::RBracket) {
                    loop {
                        elements.push(self.parse_expression()?);
                        match self.next() {
                            Some(Token::Comma) => {
                                if self.peek() == Some(&Token::RBracket) {
                                    self.index += 1;
                                    break;
                                }
                            }
                            Some(Token::RBracket) => break,
                            _ => return Err(RuntimeError::new("Unclosed [")),
                        }
                    }
                } else {
                    self.index += 1;
                }
                Ast::Array(elements)
            }
            Token::Op(op) => {
                if !matches!(op.as_str(), "!" | "-" | "+" | "~") {
                    return Err(RuntimeError::new(format!("Unexpected operator \"{op}\".")));
                }
                let argument = self.parse_token()?;
                Ast::Unary { op, argument: Box::new(argument) }
            }
            other => return Err(RuntimeError::new(format!("Unexpected {other:?}."))),
        };

        while let Some(token) = self.peek().cloned() {
            match token {
                Token::Dot => {
                    self.index += 1;
                    let property = match self.next() {
                        Some(Token::Ident(name)) => Ast::Identifier(name),
                        _ => return Err(RuntimeError::new("Expected a property name.")),
                    };
                    node = Ast::Member { computed: false, object: Box::new(node), property: Box::new(property) };
                }
                Token::LBracket => {
                    self.index += 1;
                    let property = self.parse_expression()?;
                    if self.next() != Some(Token::RBracket) {
                        return Err(RuntimeError::new("Unclosed ["));
                    }
                    node = Ast::Member { computed: true, object: Box::new(node), property: Box::new(property) };
                }
                Token::LParen => {
                    self.index += 1;
                    let mut arguments = Vec::new();
                    if self.peek() != Some(&Token::RParen) {
                        loop {
                            arguments.push(self.parse_expression()?);
                            match self.next() {
                                Some(Token::Comma) => {
                                    if self.peek() == Some(&Token::RParen) {
                                        self.index += 1;
                                        break;
                                    }
                                }
                                Some(Token::RParen) => break,
                                _ => return Err(RuntimeError::new("Unclosed (")),
                            }
                        }
                    } else {
                        self.index += 1;
                    }
                    node = Ast::Call { callee: Box::new(node), arguments };
                }
                _ => break,
            }
        }
        Ok(node)
    }
}

fn reduce_binary(operands: &mut Vec<Ast>, operators: &mut Vec<(String, i32)>) {
    let right = operands.pop().expect("right operand");
    let left = operands.pop().expect("left operand");
    let (op, _) = operators.pop().expect("operator");
    operands.push(Ast::Binary { op, left: Box::new(left), right: Box::new(right) });
}

fn binary_precedence(op: &str) -> i32 {
    match op {
        "||" | "??" => 1,
        "&&" => 2,
        "|" => 3,
        "^" => 4,
        "&" => 5,
        "==" | "!=" | "===" | "!==" => 6,
        "<" | ">" | "<=" | ">=" => 7,
        "<<" | ">>" | ">>>" => 8,
        "+" | "-" => 9,
        "*" | "/" | "%" => 10,
        "**" => 11,
        "=~" | "!~" => 0,
        _ => 0,
    }
}

fn tokenize(expression: &str) -> Result<Vec<Token>, RuntimeError> {
    let bytes = expression.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '"' | '\'' => {
                let (value, next) = gobble_string(expression, i, c)?;
                tokens.push(Token::String(value));
                i = next;
            }
            '.' => {
                if i + 1 < bytes.len() && (bytes[i + 1] as char).is_ascii_digit() {
                    let (value, next) = gobble_number(expression, i)?;
                    tokens.push(Token::Number(value));
                    i = next;
                } else {
                    tokens.push(Token::Dot);
                    i += 1;
                }
            }
            '0'..='9' => {
                let (value, next) = gobble_number(expression, i)?;
                tokens.push(Token::Number(value));
                i = next;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            '[' => {
                tokens.push(Token::LBracket);
                i += 1;
            }
            ']' => {
                tokens.push(Token::RBracket);
                i += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            '?' => {
                tokens.push(Token::Question);
                i += 1;
            }
            ':' => {
                tokens.push(Token::Colon);
                i += 1;
            }
            _ if is_identifier_start(c) => {
                let start = i;
                i += 1;
                while i < bytes.len() && is_identifier_part(bytes[i] as char) {
                    i += 1;
                }
                tokens.push(Token::Ident(expression[start..i].to_string()));
            }
            _ => {
                // Longest registered operator first, as jsep does.
                let rest = &expression[i..];
                let op = [
                    "===", "!==", "=~", "!~", "==", "!=", "<=", ">=", "&&", "||", "**", "<<", ">>", "+", "-", "*", "/",
                    "%", "<", ">", "!", "&", "|", "^",
                ]
                .iter()
                .find(|op| rest.starts_with(**op))
                .ok_or_else(|| RuntimeError::new(format!("Unexpected \"{c}\".")))?;
                tokens.push(Token::Op((*op).to_string()));
                i += op.len();
            }
        }
    }
    Ok(tokens)
}

fn is_identifier_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '$' || c == '_'
}

fn is_identifier_part(c: char) -> bool {
    is_identifier_start(c) || c.is_ascii_digit()
}

fn gobble_string(expression: &str, start: usize, quote: char) -> Result<(String, usize), RuntimeError> {
    let bytes = expression.as_bytes();
    let mut value = String::new();
    let mut i = start + 1;
    let mut closed = false;
    while i < bytes.len() {
        let c = bytes[i] as char;
        i += 1;
        if c == quote {
            closed = true;
            break;
        }
        if c == '\\' {
            // Cesium removes backslashes before parsing, so this path only
            // exists for direct parser use.
            if i < bytes.len() {
                let escaped = bytes[i] as char;
                i += 1;
                value.push(match escaped {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'b' => '\u{8}',
                    'f' => '\u{c}',
                    'v' => '\u{b}',
                    other => other,
                });
            }
        } else {
            value.push(c);
        }
    }
    if !closed {
        return Err(RuntimeError::new(format!("Unclosed quote after \"{value}\"")));
    }
    Ok((value, i))
}

fn gobble_number(expression: &str, start: usize) -> Result<(f64, usize), RuntimeError> {
    let bytes = expression.as_bytes();
    let mut i = start;
    while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
            i += 1;
        }
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        let exponent_start = i;
        while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
            i += 1;
        }
        if i == exponent_start {
            return Err(RuntimeError::new(format!("Expected exponent ({})", &expression[start..i])));
        }
    }
    let text = &expression[start..i];
    if i < bytes.len() && is_identifier_start(bytes[i] as char) {
        return Err(RuntimeError::new(format!("Variable names cannot start with a number ({text})")));
    }
    let value = text.parse::<f64>().map_err(|_| RuntimeError::new(format!("Unexpected number {text}")))?;
    Ok((value, i))
}

// ---------------------------------------------------------------------------
// Runtime AST
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum NodeKind {
    Variable,
    Unary,
    Binary,
    Ternary,
    Conditional,
    Member,
    FunctionCall,
    Array,
    Regex,
    VariableInString,
    LiteralNull,
    LiteralBoolean,
    LiteralNumber,
    LiteralString,
    LiteralColor,
    LiteralVector,
    LiteralRegex,
    LiteralUndefined,
    BuiltinVariable,
}

enum NodeValue {
    None,
    Boolean(bool),
    Number(f64),
    String(String),
    Nodes(Vec<Node>),
}

struct Node {
    kind: NodeKind,
    value: NodeValue,
    left: Option<Box<Node>>,
    right: Option<Box<Node>>,
    test: Option<Box<Node>>,
    /// The pattern child of a dynamic `regExp(pattern, flags)` node. It is not
    /// part of `left`, matching upstream's `value` being an AST node.
    regex_pattern: Option<Box<Node>>,
    literal_regex: Option<Rc<RefCell<RegExpValue>>>,
    children: Vec<Node>,
}

impl Node {
    fn new(kind: NodeKind, value: NodeValue) -> Self {
        Self {
            kind,
            value,
            left: None,
            right: None,
            test: None,
            regex_pattern: None,
            literal_regex: None,
            children: Vec::new(),
        }
    }

    fn with_left(mut self, left: Node) -> Self {
        self.left = Some(Box::new(left));
        self
    }

    fn with_right(mut self, right: Node) -> Self {
        self.right = Some(Box::new(right));
        self
    }

    fn with_test(mut self, test: Node) -> Self {
        self.test = Some(Box::new(test));
        self
    }

    fn with_children(mut self, children: Vec<Node>) -> Self {
        self.children = children;
        self
    }

    fn string_value(&self) -> &str {
        match &self.value {
            NodeValue::String(s) => s,
            _ => "",
        }
    }

    fn children(&self) -> &[Node] {
        if !self.children.is_empty() {
            &self.children
        } else {
            match &self.value {
                NodeValue::Nodes(nodes) => nodes,
                _ => &[],
            }
        }
    }

    fn is_feature(&self) -> bool {
        self.kind == NodeKind::Variable && self.string_value() == "feature"
    }

    fn is_literal(&self) -> bool {
        self.kind >= NodeKind::LiteralNull
    }
}

fn unary_function_names() -> &'static [&'static str] {
    &[
        "abs",
        "sqrt",
        "cos",
        "sin",
        "tan",
        "acos",
        "asin",
        "atan",
        "radians",
        "degrees",
        "sign",
        "floor",
        "ceil",
        "round",
        "exp",
        "exp2",
        "log",
        "log2",
        "fract",
        "length",
        "normalize",
    ]
}

fn binary_function_names() -> &'static [&'static str] {
    &["atan2", "pow", "min", "max", "distance", "dot", "cross"]
}

fn ternary_function_names() -> &'static [&'static str] {
    &["clamp", "mix"]
}

fn unary_operators() -> &'static [&'static str] {
    &["!", "-", "+"]
}

fn binary_operators() -> &'static [&'static str] {
    &["+", "-", "*", "/", "%", "===", "!==", ">", ">=", "<", "<=", "&&", "||", "!~", "=~"]
}

const VARIABLE_SENTINEL: &str = "@#%";

fn replace_defines(expression: &str, defines: Option<&BTreeMap<String, String>>) -> String {
    let Some(defines) = defines else {
        return expression.to_string();
    };
    let mut expression = expression.to_string();
    for (key, value) in defines {
        expression = expression.replace(&format!("${{{key}}}"), &format!("({value})"));
    }
    expression
}

fn remove_backslashes(expression: &str) -> String {
    expression.replace('\\', VARIABLE_SENTINEL)
}

fn replace_backslashes(expression: &str) -> String {
    expression.replace(VARIABLE_SENTINEL, "\\")
}

/// Replaces `${name}` outside quoted strings with the identifier `czm_name`.
fn replace_variables(expression: &str) -> Result<String, RuntimeError> {
    let mut exp = expression.to_string();
    let mut result = String::new();
    let mut i = exp.find("${");
    while let Some(position) = i {
        let open_single = exp.find('\'');
        let open_double = exp.find('"');
        if let Some(open) = open_single.filter(|open| *open < position) {
            let Some(close) = exp[open + 1..].find('\'').map(|c| c + open + 1) else {
                result.push_str(&exp);
                return Ok(result);
            };
            result.push_str(&exp[..=close]);
            exp = exp[close + 1..].to_string();
        } else if let Some(open) = open_double.filter(|open| *open < position) {
            let Some(close) = exp[open + 1..].find('"').map(|c| c + open + 1) else {
                result.push_str(&exp);
                return Ok(result);
            };
            result.push_str(&exp[..=close]);
            exp = exp[close + 1..].to_string();
        } else {
            result.push_str(&exp[..position]);
            let Some(close) = exp[position..].find('}').map(|c| c + position) else {
                return Err(RuntimeError::new("Unmatched {."));
            };
            result.push_str("czm_");
            result.push_str(&exp[position + 2..close]);
            exp = exp[close + 1..].to_string();
        }
        i = exp.find("${");
    }
    result.push_str(&exp);
    Ok(result)
}

fn create_runtime_ast(ast: &Ast) -> Result<Node, RuntimeError> {
    match ast {
        Ast::Literal(literal) => Ok(parse_literal(literal)),
        Ast::Call { callee, arguments } => parse_call(callee, arguments),
        Ast::Identifier(name) => parse_keyword_or_variable(name),
        Ast::Unary { op, argument } => {
            if !unary_operators().contains(&op.as_str()) {
                return Err(RuntimeError::new(format!("Unexpected operator \"{op}\".")));
            }
            Ok(Node::new(NodeKind::Unary, NodeValue::String(op.clone())).with_left(create_runtime_ast(argument)?))
        }
        Ast::Binary { op, left, right } => {
            if !binary_operators().contains(&op.as_str()) {
                return Err(RuntimeError::new(format!("Unexpected operator \"{op}\".")));
            }
            Ok(Node::new(NodeKind::Binary, NodeValue::String(op.clone()))
                .with_left(create_runtime_ast(left)?)
                .with_right(create_runtime_ast(right)?))
        }
        Ast::Conditional { test, consequent, alternate } => {
            Ok(Node::new(NodeKind::Conditional, NodeValue::String("?".into()))
                .with_left(create_runtime_ast(consequent)?)
                .with_right(create_runtime_ast(alternate)?)
                .with_test(create_runtime_ast(test)?))
        }
        Ast::Member { computed, object, property } => parse_member_expression(*computed, object, property),
        Ast::Array(elements) => {
            let nodes = elements.iter().map(create_runtime_ast).collect::<Result<Vec<_>, _>>()?;
            Ok(Node::new(NodeKind::Array, NodeValue::Nodes(nodes)))
        }
        Ast::Compound => Err(RuntimeError::new("Provide exactly one expression.")),
    }
}

fn parse_literal(literal: &Literal) -> Node {
    match literal {
        Literal::Null => Node::new(NodeKind::LiteralNull, NodeValue::None),
        Literal::Boolean(value) => Node::new(NodeKind::LiteralBoolean, NodeValue::Boolean(*value)),
        Literal::Number(value) => Node::new(NodeKind::LiteralNumber, NodeValue::Number(*value)),
        Literal::String(value) => {
            if value.contains("${") {
                Node::new(NodeKind::VariableInString, NodeValue::String(value.clone()))
            } else {
                Node::new(NodeKind::LiteralString, NodeValue::String(replace_backslashes(value)))
            }
        }
    }
}

fn parse_keyword_or_variable(name: &str) -> Result<Node, RuntimeError> {
    if let Some(variable) = name.strip_prefix("czm_") {
        if variable.starts_with("tiles3d_") {
            return Ok(Node::new(NodeKind::BuiltinVariable, NodeValue::String(variable.to_string())));
        }
        return Ok(Node::new(NodeKind::Variable, NodeValue::String(variable.to_string())));
    }
    match name {
        "NaN" => Ok(Node::new(NodeKind::LiteralNumber, NodeValue::Number(f64::NAN))),
        "Infinity" => Ok(Node::new(NodeKind::LiteralNumber, NodeValue::Number(f64::INFINITY))),
        "undefined" => Ok(Node::new(NodeKind::LiteralUndefined, NodeValue::None)),
        other => Err(RuntimeError::new(format!("{other} is not defined."))),
    }
}

fn parse_member_expression(computed: bool, object: &Ast, property: &Ast) -> Result<Node, RuntimeError> {
    if let Ast::Identifier(name) = object {
        if name == "Math" {
            if let Ast::Identifier(property) = property {
                return match property.as_str() {
                    "PI" => Ok(Node::new(NodeKind::LiteralNumber, NodeValue::Number(std::f64::consts::PI))),
                    "E" => Ok(Node::new(NodeKind::LiteralNumber, NodeValue::Number(std::f64::consts::E))),
                    other => Err(RuntimeError::new(format!("Math.{other} is not defined."))),
                };
            }
        } else if name == "Number" {
            if let Ast::Identifier(property) = property {
                return match property.as_str() {
                    "POSITIVE_INFINITY" => Ok(Node::new(NodeKind::LiteralNumber, NodeValue::Number(f64::INFINITY))),
                    other => Err(RuntimeError::new(format!("Number.{other} is not defined."))),
                };
            }
        }
    }

    let object = create_runtime_ast(object)?;
    if computed {
        return Ok(Node::new(NodeKind::Member, NodeValue::String("brackets".into()))
            .with_left(object)
            .with_right(create_runtime_ast(property)?));
    }
    let Ast::Identifier(property) = property else {
        return Err(RuntimeError::new("Expected a property name."));
    };
    Ok(Node::new(NodeKind::Member, NodeValue::String("dot".into()))
        .with_left(object)
        .with_right(Node::new(NodeKind::LiteralString, NodeValue::String(property.clone()))))
}

fn parse_call(callee: &Ast, arguments: &[Ast]) -> Result<Node, RuntimeError> {
    let args_length = arguments.len();
    if let Ast::Member { property, object, .. } = callee {
        let Ast::Identifier(call) = property.as_ref() else {
            return Err(RuntimeError::new("Unexpected function call."));
        };
        if call == "test" || call == "exec" {
            let Ast::Call { callee: object_callee, .. } = object.as_ref() else {
                return Err(RuntimeError::new(format!("{call} is not a function.")));
            };
            let Ast::Identifier(object_name) = object_callee.as_ref() else {
                return Err(RuntimeError::new(format!("{call} is not a function.")));
            };
            if object_name != "regExp" {
                return Err(RuntimeError::new(format!("{call} is not a function.")));
            }
            if args_length == 0 {
                return Ok(if call == "test" {
                    Node::new(NodeKind::LiteralBoolean, NodeValue::Boolean(false))
                } else {
                    Node::new(NodeKind::LiteralNull, NodeValue::None)
                });
            }
            let left = create_runtime_ast(object)?;
            let right = create_runtime_ast(&arguments[0])?;
            return Ok(Node::new(NodeKind::FunctionCall, NodeValue::String(call.clone()))
                .with_left(left)
                .with_right(right));
        }
        if call == "toString" {
            let value = create_runtime_ast(object)?;
            return Ok(Node::new(NodeKind::FunctionCall, NodeValue::String(call.clone())).with_left(value));
        }
        return Err(RuntimeError::new(format!("Unexpected function call \"{call}\".")));
    }

    let Ast::Identifier(call) = callee else {
        return Err(RuntimeError::new("Unexpected function call."));
    };
    match call.as_str() {
        "color" => {
            let mut node = Node::new(NodeKind::LiteralColor, NodeValue::String(call.clone()));
            if args_length > 0 {
                let mut children = vec![create_runtime_ast(&arguments[0])?];
                if let Some(alpha) = arguments.get(1) {
                    children.push(create_runtime_ast(alpha)?);
                }
                node.children = children;
            }
            Ok(node)
        }
        "rgb" | "hsl" => {
            if args_length < 3 {
                return Err(RuntimeError::new(format!("{call} requires three arguments.")));
            }
            let children = arguments[..3].iter().map(create_runtime_ast).collect::<Result<Vec<_>, _>>()?;
            Ok(Node::new(NodeKind::LiteralColor, NodeValue::String(call.clone())).with_children(children))
        }
        "rgba" | "hsla" => {
            if args_length < 4 {
                return Err(RuntimeError::new(format!("{call} requires four arguments.")));
            }
            let children = arguments[..4].iter().map(create_runtime_ast).collect::<Result<Vec<_>, _>>()?;
            Ok(Node::new(NodeKind::LiteralColor, NodeValue::String(call.clone())).with_children(children))
        }
        "vec2" | "vec3" | "vec4" => {
            let children = arguments.iter().map(create_runtime_ast).collect::<Result<Vec<_>, _>>()?;
            Ok(Node::new(NodeKind::LiteralVector, NodeValue::String(call.clone())).with_children(children))
        }
        "isNaN" | "isFinite" => {
            if args_length == 0 {
                return Ok(Node::new(NodeKind::LiteralBoolean, NodeValue::Boolean(call == "isNaN")));
            }
            Ok(Node::new(NodeKind::Unary, NodeValue::String(call.clone()))
                .with_left(create_runtime_ast(&arguments[0])?))
        }
        "isExactClass" | "isClass" => {
            if args_length != 1 {
                return Err(RuntimeError::new(format!("{call} requires exactly one argument.")));
            }
            Ok(Node::new(NodeKind::Unary, NodeValue::String(call.clone()))
                .with_left(create_runtime_ast(&arguments[0])?))
        }
        "getExactClassName" => {
            if args_length > 0 {
                return Err(RuntimeError::new(format!("{call} does not take any argument.")));
            }
            Ok(Node::new(NodeKind::Unary, NodeValue::String(call.clone())))
        }
        _ if unary_function_names().contains(&call.as_str()) => {
            if args_length != 1 {
                return Err(RuntimeError::new(format!("{call} requires exactly one argument.")));
            }
            Ok(Node::new(NodeKind::Unary, NodeValue::String(call.clone()))
                .with_left(create_runtime_ast(&arguments[0])?))
        }
        _ if binary_function_names().contains(&call.as_str()) => {
            if args_length != 2 {
                return Err(RuntimeError::new(format!("{call} requires exactly two arguments.")));
            }
            Ok(Node::new(NodeKind::Binary, NodeValue::String(call.clone()))
                .with_left(create_runtime_ast(&arguments[0])?)
                .with_right(create_runtime_ast(&arguments[1])?))
        }
        _ if ternary_function_names().contains(&call.as_str()) => {
            if args_length != 3 {
                return Err(RuntimeError::new(format!("{call} requires exactly three arguments.")));
            }
            Ok(Node::new(NodeKind::Ternary, NodeValue::String(call.clone()))
                .with_left(create_runtime_ast(&arguments[0])?)
                .with_right(create_runtime_ast(&arguments[1])?)
                .with_test(create_runtime_ast(&arguments[2])?))
        }
        "Boolean" | "Number" | "String" => {
            if args_length == 0 {
                let node = match call.as_str() {
                    "Boolean" => Node::new(NodeKind::LiteralBoolean, NodeValue::Boolean(false)),
                    "Number" => Node::new(NodeKind::LiteralNumber, NodeValue::Number(0.0)),
                    _ => Node::new(NodeKind::LiteralString, NodeValue::String(String::new())),
                };
                return Ok(node);
            }
            Ok(Node::new(NodeKind::Unary, NodeValue::String(call.clone()))
                .with_left(create_runtime_ast(&arguments[0])?))
        }
        "regExp" => parse_regex(arguments),
        other => Err(RuntimeError::new(format!("Unexpected function call \"{other}\"."))),
    }
}

fn parse_regex(arguments: &[Ast]) -> Result<Node, RuntimeError> {
    if arguments.is_empty() {
        let regex = RegExpValue::new("", "")?;
        let mut node = Node::new(NodeKind::LiteralRegex, NodeValue::None);
        node.literal_regex = Some(Rc::new(RefCell::new(regex)));
        return Ok(node);
    }

    let pattern = create_runtime_ast(&arguments[0])?;
    if let Some(flags_ast) = arguments.get(1) {
        let flags = create_runtime_ast(flags_ast)?;
        if pattern.is_literal() && flags.is_literal() {
            let pattern_text = literal_text(&pattern);
            let NodeValue::String(flags_text) = &flags.value else {
                return Err(RuntimeError::new("Invalid regular expression flags."));
            };
            let regex = RegExpValue::new(pattern_text, flags_text.clone())?;
            let mut node = Node::new(NodeKind::LiteralRegex, NodeValue::None);
            node.literal_regex = Some(Rc::new(RefCell::new(regex)));
            return Ok(node);
        }
        let mut node = Node::new(NodeKind::Regex, NodeValue::None);
        node.regex_pattern = Some(Box::new(pattern));
        node.left = Some(Box::new(flags));
        return Ok(node);
    }

    if pattern.is_literal() {
        let regex = RegExpValue::new(literal_text(&pattern), "")?;
        let mut node = Node::new(NodeKind::LiteralRegex, NodeValue::None);
        node.literal_regex = Some(Rc::new(RefCell::new(regex)));
        return Ok(node);
    }
    let mut node = Node::new(NodeKind::Regex, NodeValue::None);
    node.regex_pattern = Some(Box::new(pattern));
    Ok(node)
}

fn literal_text(node: &Node) -> String {
    match &node.value {
        NodeValue::String(s) => s.clone(),
        NodeValue::Number(n) => js_number_to_string(*n),
        NodeValue::Boolean(b) => b.to_string(),
        NodeValue::None => "null".into(),
        NodeValue::Nodes(_) => node.js_value_string(),
    }
}

impl Node {
    fn js_value_string(&self) -> String {
        match &self.value {
            NodeValue::String(s) => s.clone(),
            NodeValue::Number(n) => js_number_to_string(*n),
            NodeValue::Boolean(b) => b.to_string(),
            _ => String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

impl Node {
    fn evaluate(&self, feature: Option<&dyn StyleFeature>) -> Result<StyleValue, RuntimeError> {
        use NodeKind::*;
        match self.kind {
            LiteralNull => Ok(StyleValue::Null),
            LiteralUndefined => Ok(StyleValue::Undefined),
            LiteralBoolean => match &self.value {
                NodeValue::Boolean(b) => Ok(StyleValue::Boolean(*b)),
                _ => Ok(StyleValue::Undefined),
            },
            LiteralNumber => match &self.value {
                NodeValue::Number(n) => Ok(StyleValue::Number(*n)),
                _ => Ok(StyleValue::Undefined),
            },
            LiteralString | VariableInString => match self.kind {
                LiteralString => match &self.value {
                    NodeValue::String(s) => Ok(StyleValue::String(s.clone())),
                    _ => Ok(StyleValue::Undefined),
                },
                _ => self.evaluate_variable_string(feature),
            },
            LiteralColor => self.evaluate_literal_color(feature),
            LiteralVector => self.evaluate_literal_vector(feature),
            LiteralRegex => Ok(StyleValue::RegExp(Rc::clone(self.literal_regex.as_ref().expect("literal regex")))),
            Variable => {
                let name = self.string_value();
                Ok(feature.and_then(|feature| feature.property_inherited(name)).unwrap_or(StyleValue::Undefined))
            }
            BuiltinVariable => {
                if self.string_value() == "tiles3d_tileset_time" {
                    Ok(StyleValue::Number(feature.map_or(0.0, |f| f.tileset_time())))
                } else {
                    Ok(StyleValue::Undefined)
                }
            }
            Array => Ok(StyleValue::Array(
                self.children().iter().map(|node| node.evaluate(feature)).collect::<Result<_, _>>()?,
            )),
            Regex => {
                let pattern = self.regex_pattern.as_ref().expect("regex pattern").evaluate(feature)?;
                let flags = match &self.left {
                    Some(flags) => flags.evaluate(feature)?,
                    None => StyleValue::String(String::new()),
                };
                let flags = match flags {
                    StyleValue::String(flags) => flags,
                    other => other.js_string(),
                };
                Ok(StyleValue::RegExp(Rc::new(RefCell::new(RegExpValue::new(pattern.js_string(), flags)?))))
            }
            FunctionCall => self.evaluate_function_call(feature),
            Unary => self.evaluate_unary(feature),
            Binary => self.evaluate_binary(feature),
            Ternary => self.evaluate_ternary(feature),
            Conditional => {
                let test = self.test.as_ref().expect("conditional test").evaluate(feature)?;
                let StyleValue::Boolean(test) = test else {
                    return Err(RuntimeError::new(format!(
                        "Conditional argument of conditional expression must be a boolean. Argument is {}.",
                        test.js_string()
                    )));
                };
                if test {
                    self.left.as_ref().expect("consequent").evaluate(feature)
                } else {
                    self.right.as_ref().expect("alternate").evaluate(feature)
                }
            }
            Member => self.evaluate_member(feature),
        }
    }

    fn evaluate_literal_color(&self, feature: Option<&dyn StyleFeature>) -> Result<StyleValue, RuntimeError> {
        let args = self.children();
        let call = self.string_value();
        let color = match call {
            "color" => {
                if args.is_empty() {
                    Color::from_bytes(255.0, 255.0, 255.0, 255.0)
                } else {
                    let value = args[0].evaluate(feature)?;
                    let mut color = Color::from_css_color_string(&value.js_string())
                        .ok_or_else(|| RuntimeError::new("Invalid color"))?;
                    if args.len() > 1 {
                        color.alpha = args[1].evaluate(feature)?.to_number();
                    }
                    color
                }
            }
            "rgb" => Color::from_bytes(
                args[0].evaluate(feature)?.to_number(),
                args[1].evaluate(feature)?.to_number(),
                args[2].evaluate(feature)?.to_number(),
                255.0,
            ),
            "rgba" => Color::from_bytes(
                args[0].evaluate(feature)?.to_number(),
                args[1].evaluate(feature)?.to_number(),
                args[2].evaluate(feature)?.to_number(),
                args[3].evaluate(feature)?.to_number() * 255.0,
            ),
            "hsl" => Color::from_hsl(
                args[0].evaluate(feature)?.to_number(),
                args[1].evaluate(feature)?.to_number(),
                args[2].evaluate(feature)?.to_number(),
                1.0,
            ),
            "hsla" => Color::from_hsl(
                args[0].evaluate(feature)?.to_number(),
                args[1].evaluate(feature)?.to_number(),
                args[2].evaluate(feature)?.to_number(),
                args[3].evaluate(feature)?.to_number(),
            ),
            _ => return Err(RuntimeError::new(format!("Unexpected function call \"{call}\"."))),
        };
        Ok(StyleValue::Cartesian4(Cartesian4::from_color(&color)))
    }

    fn evaluate_literal_vector(&self, feature: Option<&dyn StyleFeature>) -> Result<StyleValue, RuntimeError> {
        let call = self.string_value();
        let args = self.children();
        let mut components = Vec::new();
        for argument in args {
            match argument.evaluate(feature)? {
                StyleValue::Number(n) => components.push(n),
                StyleValue::Cartesian2(v) => components.extend([v.x, v.y]),
                StyleValue::Cartesian3(v) => components.extend([v.x, v.y, v.z]),
                StyleValue::Cartesian4(v) => components.extend([v.x, v.y, v.z, v.w]),
                other => {
                    return Err(RuntimeError::new(format!(
                        "{call} argument must be a vector or number. Argument is {}.",
                        other.js_string()
                    )))
                }
            }
        }
        let vector_length: usize = call[3..].parse().unwrap_or(2);
        if components.is_empty() {
            return Err(RuntimeError::new(format!("Invalid {call} constructor. No valid arguments.")));
        }
        if components.len() < vector_length && components.len() > 1 {
            return Err(RuntimeError::new(format!("Invalid {call} constructor. Not enough arguments.")));
        }
        if components.len() > vector_length && args.len() > 1 {
            return Err(RuntimeError::new(format!("Invalid {call} constructor. Too many arguments.")));
        }
        if components.len() == 1 {
            let component = components[0];
            components.extend([component, component, component]);
        }
        Ok(match call {
            "vec2" => StyleValue::Cartesian2(Cartesian2::new(components[0], components[1])),
            "vec3" => StyleValue::Cartesian3(Cartesian3::new(components[0], components[1], components[2])),
            _ => StyleValue::Cartesian4(Cartesian4::new(components[0], components[1], components[2], components[3])),
        })
    }

    fn evaluate_variable_string(&self, feature: Option<&dyn StyleFeature>) -> Result<StyleValue, RuntimeError> {
        let mut text = self.string_value().to_string();
        while let Some(start) = text.find("${") {
            let Some(close) = text[start..].find('}').map(|c| c + start) else {
                break;
            };
            let name = &text[start + 2..close];
            let property = feature
                .and_then(|feature| feature.property_inherited(name))
                .unwrap_or_else(|| StyleValue::String(String::new()));
            let property = property.js_string();
            text.replace_range(start..=close, &property);
        }
        Ok(StyleValue::String(text))
    }

    fn evaluate_function_call(&self, feature: Option<&dyn StyleFeature>) -> Result<StyleValue, RuntimeError> {
        let call = self.string_value();
        let left = self.left.as_ref().map(|left| left.evaluate(feature)).transpose()?;
        let right = self.right.as_ref().map(|right| right.evaluate(feature)).transpose()?;
        match call {
            "test" => match (left, right) {
                (Some(StyleValue::RegExp(regex)), Some(StyleValue::String(input))) => {
                    let matched = regex.borrow_mut().test(&input)?;
                    Ok(StyleValue::Boolean(matched))
                }
                (left, right) => Err(RuntimeError::new(format!(
                    "RegExp.test requires the first argument to be a RegExp and the second argument to be a string. Arguments are {} and {}.",
                    left.map_or_else(|| "undefined".to_string(), |v| v.js_string()),
                    right.map_or_else(|| "undefined".to_string(), |v| v.js_string())
                ))),
            },
            "exec" => match (left, right) {
                (Some(StyleValue::RegExp(regex)), Some(StyleValue::String(input))) => {
                    let result = regex.borrow_mut().exec_group1(&input)?;
                    Ok(match result {
                        StyleValue::Undefined => StyleValue::Null,
                        other => other,
                    })
                }
                (left, right) => Err(RuntimeError::new(format!(
                    "RegExp.exec requires the first argument to be a RegExp and the second argument to be a string. Arguments are {} and {}.",
                    left.map_or_else(|| "undefined".to_string(), |v| v.js_string()),
                    right.map_or_else(|| "undefined".to_string(), |v| v.js_string())
                ))),
            },
            "toString" => {
                let value = left.unwrap_or(StyleValue::Undefined);
                if matches!(value, StyleValue::RegExp(_))
                    || matches!(value, StyleValue::Cartesian2(_) | StyleValue::Cartesian3(_) | StyleValue::Cartesian4(_))
                {
                    Ok(StyleValue::String(value.js_string()))
                } else {
                    Err(RuntimeError::new(format!("Unexpected function call \"{call}\".")))
                }
            }
            _ => Err(RuntimeError::new(format!("Unexpected function call \"{call}\"."))),
        }
    }

    fn evaluate_unary(&self, feature: Option<&dyn StyleFeature>) -> Result<StyleValue, RuntimeError> {
        let call = self.string_value();
        let left = match &self.left {
            Some(left) => Some(left.evaluate(feature)?),
            None => None,
        };
        match call {
            "!" => match left.unwrap_or(StyleValue::Undefined) {
                StyleValue::Boolean(b) => Ok(StyleValue::Boolean(!b)),
                other => Err(RuntimeError::new(format!(
                    "Operator \"!\" requires a boolean argument. Argument is {}.",
                    other.js_string()
                ))),
            },
            "-" => match left.unwrap_or(StyleValue::Undefined) {
                StyleValue::Cartesian2(v) => Ok(StyleValue::Cartesian2(v.negate())),
                StyleValue::Cartesian3(v) => Ok(StyleValue::Cartesian3(v.negate())),
                StyleValue::Cartesian4(v) => Ok(StyleValue::Cartesian4(v.negate())),
                StyleValue::Number(n) => Ok(StyleValue::Number(-n)),
                other => Err(RuntimeError::new(format!(
                    "Operator \"-\" requires a vector or number argument. Argument is {}.",
                    other.js_string()
                ))),
            },
            "+" => {
                let value = left.unwrap_or(StyleValue::Undefined);
                if value.is_vector() || matches!(value, StyleValue::Number(_)) {
                    Ok(value)
                } else {
                    Err(RuntimeError::new(format!(
                        "Operator \"+\" requires a vector or number argument. Argument is {}.",
                        value.js_string()
                    )))
                }
            }
            "isNaN" => Ok(StyleValue::Boolean(left.unwrap_or(StyleValue::Undefined).to_number().is_nan())),
            "isFinite" => Ok(StyleValue::Boolean(left.unwrap_or(StyleValue::Undefined).to_number().is_finite())),
            "isExactClass" => {
                let class = left.unwrap_or(StyleValue::Undefined).js_string();
                Ok(StyleValue::Boolean(feature.is_some_and(|f| f.is_exact_class(&class))))
            }
            "isClass" => {
                let class = left.unwrap_or(StyleValue::Undefined).js_string();
                Ok(StyleValue::Boolean(feature.is_some_and(|f| f.is_class(&class))))
            }
            "getExactClassName" => {
                Ok(feature.and_then(|f| f.exact_class_name()).map(StyleValue::String).unwrap_or(StyleValue::Undefined))
            }
            "Boolean" => Ok(StyleValue::Boolean(left.unwrap_or(StyleValue::Undefined).to_boolean())),
            "Number" => Ok(StyleValue::Number(left.unwrap_or(StyleValue::Undefined).to_number())),
            "String" => Ok(StyleValue::String(left.unwrap_or(StyleValue::Undefined).js_string())),
            _ if unary_function_names().contains(&call) => {
                evaluate_unary_componentwise(call, left.unwrap_or(StyleValue::Undefined))
            }
            _ => Err(RuntimeError::new(format!("Unexpected function call \"{call}\"."))),
        }
    }

    fn evaluate_binary(&self, feature: Option<&dyn StyleFeature>) -> Result<StyleValue, RuntimeError> {
        let call = self.string_value();
        let left = self.left.as_ref().expect("binary left").evaluate(feature)?;
        let right = self.right.as_ref().expect("binary right").evaluate(feature)?;
        match call {
            "+" => match (&left, &right) {
                (StyleValue::Cartesian2(a), StyleValue::Cartesian2(b)) => Ok(StyleValue::Cartesian2(a.add(*b))),
                (StyleValue::Cartesian3(a), StyleValue::Cartesian3(b)) => Ok(StyleValue::Cartesian3(a.add(*b))),
                (StyleValue::Cartesian4(a), StyleValue::Cartesian4(b)) => Ok(StyleValue::Cartesian4(a.add(*b))),
                (StyleValue::String(_), _) | (_, StyleValue::String(_)) => {
                    Ok(StyleValue::String(format!("{}{}", left.js_string(), right.js_string())))
                }
                (StyleValue::Number(a), StyleValue::Number(b)) => Ok(StyleValue::Number(a + b)),
                _ => Err(RuntimeError::new(format!(
                    "Operator \"+\" requires vector or number arguments of matching types, or at least one string argument. Arguments are {} and {}.",
                    left.js_string(),
                    right.js_string()
                ))),
            },
            "-" => match (&left, &right) {
                (StyleValue::Cartesian2(a), StyleValue::Cartesian2(b)) => Ok(StyleValue::Cartesian2(a.subtract(*b))),
                (StyleValue::Cartesian3(a), StyleValue::Cartesian3(b)) => Ok(StyleValue::Cartesian3(a.subtract(*b))),
                (StyleValue::Cartesian4(a), StyleValue::Cartesian4(b)) => Ok(StyleValue::Cartesian4(a.subtract(*b))),
                (StyleValue::Number(a), StyleValue::Number(b)) => Ok(StyleValue::Number(a - b)),
                _ => Err(RuntimeError::new(format!(
                    "Operator \"-\" requires vector or number arguments of matching types. Arguments are {} and {}.",
                    left.js_string(),
                    right.js_string()
                ))),
            },
            "*" => match (&left, &right) {
                (StyleValue::Cartesian2(a), StyleValue::Cartesian2(b)) => {
                    Ok(StyleValue::Cartesian2(a.multiply_components(*b)))
                }
                (StyleValue::Number(a), StyleValue::Cartesian2(b)) => Ok(StyleValue::Cartesian2(b.multiply_by_scalar(*a))),
                (StyleValue::Cartesian2(a), StyleValue::Number(b)) => Ok(StyleValue::Cartesian2(a.multiply_by_scalar(*b))),
                (StyleValue::Cartesian3(a), StyleValue::Cartesian3(b)) => {
                    Ok(StyleValue::Cartesian3(a.multiply_components(*b)))
                }
                (StyleValue::Number(a), StyleValue::Cartesian3(b)) => Ok(StyleValue::Cartesian3(b.multiply_by_scalar(*a))),
                (StyleValue::Cartesian3(a), StyleValue::Number(b)) => Ok(StyleValue::Cartesian3(a.multiply_by_scalar(*b))),
                (StyleValue::Cartesian4(a), StyleValue::Cartesian4(b)) => {
                    Ok(StyleValue::Cartesian4(a.multiply_components(*b)))
                }
                (StyleValue::Number(a), StyleValue::Cartesian4(b)) => Ok(StyleValue::Cartesian4(b.multiply_by_scalar(*a))),
                (StyleValue::Cartesian4(a), StyleValue::Number(b)) => Ok(StyleValue::Cartesian4(a.multiply_by_scalar(*b))),
                (StyleValue::Number(a), StyleValue::Number(b)) => Ok(StyleValue::Number(a * b)),
                _ => Err(RuntimeError::new(format!(
                    "Operator \"*\" requires vector or number arguments. If both arguments are vectors they must be matching types. Arguments are {} and {}.",
                    left.js_string(),
                    right.js_string()
                ))),
            },
            "/" => match (&left, &right) {
                (StyleValue::Cartesian2(a), StyleValue::Cartesian2(b)) => {
                    Ok(StyleValue::Cartesian2(a.divide_components(*b)))
                }
                (StyleValue::Cartesian2(a), StyleValue::Number(b)) => Ok(StyleValue::Cartesian2(a.divide_by_scalar(*b))),
                (StyleValue::Cartesian3(a), StyleValue::Cartesian3(b)) => {
                    Ok(StyleValue::Cartesian3(a.divide_components(*b)))
                }
                (StyleValue::Cartesian3(a), StyleValue::Number(b)) => Ok(StyleValue::Cartesian3(a.divide_by_scalar(*b))),
                (StyleValue::Cartesian4(a), StyleValue::Cartesian4(b)) => {
                    Ok(StyleValue::Cartesian4(a.divide_components(*b)))
                }
                (StyleValue::Cartesian4(a), StyleValue::Number(b)) => Ok(StyleValue::Cartesian4(a.divide_by_scalar(*b))),
                (StyleValue::Number(a), StyleValue::Number(b)) => Ok(StyleValue::Number(a / b)),
                _ => Err(RuntimeError::new(format!(
                    "Operator \"/\" requires vector or number arguments of matching types, or a number as the second argument. Arguments are {} and {}.",
                    left.js_string(),
                    right.js_string()
                ))),
            },
            "%" => match (&left, &right) {
                (StyleValue::Cartesian2(a), StyleValue::Cartesian2(b)) => Ok(StyleValue::Cartesian2(Cartesian2::new(
                    js_mod(a.x, b.x),
                    js_mod(a.y, b.y),
                ))),
                (StyleValue::Cartesian3(a), StyleValue::Cartesian3(b)) => Ok(StyleValue::Cartesian3(Cartesian3::new(
                    js_mod(a.x, b.x),
                    js_mod(a.y, b.y),
                    js_mod(a.z, b.z),
                ))),
                (StyleValue::Cartesian4(a), StyleValue::Cartesian4(b)) => Ok(StyleValue::Cartesian4(Cartesian4::new(
                    js_mod(a.x, b.x),
                    js_mod(a.y, b.y),
                    js_mod(a.z, b.z),
                    js_mod(a.w, b.w),
                ))),
                (StyleValue::Number(a), StyleValue::Number(b)) => Ok(StyleValue::Number(js_mod(*a, *b))),
                _ => Err(RuntimeError::new(format!(
                    "Operator \"%\" requires vector or number arguments of matching types. Arguments are {} and {}.",
                    left.js_string(),
                    right.js_string()
                ))),
            },
            "===" => Ok(StyleValue::Boolean(left.strict_equals(&right))),
            "!==" => Ok(StyleValue::Boolean(!left.strict_equals(&right))),
            "<" | "<=" | ">" | ">=" => {
                let (StyleValue::Number(a), StyleValue::Number(b)) = (&left, &right) else {
                    return Err(RuntimeError::new(format!(
                        "Operator \"{call}\" requires number arguments. Arguments are {} and {}.",
                        left.js_string(),
                        right.js_string()
                    )));
                };
                Ok(StyleValue::Boolean(match call {
                    "<" => a < b,
                    "<=" => a <= b,
                    ">" => a > b,
                    _ => a >= b,
                }))
            }
            "&&" => {
                let StyleValue::Boolean(a) = left else {
                    return Err(RuntimeError::new(format!(
                        "Operator \"&&\" requires boolean arguments. First argument is {}.",
                        left.js_string()
                    )));
                };
                if !a {
                    return Ok(StyleValue::Boolean(false));
                }
                match right {
                    StyleValue::Boolean(b) => Ok(StyleValue::Boolean(b)),
                    other => Err(RuntimeError::new(format!(
                        "Operator \"&&\" requires boolean arguments. Second argument is {}.",
                        other.js_string()
                    ))),
                }
            }
            "||" => {
                let StyleValue::Boolean(a) = left else {
                    return Err(RuntimeError::new(format!(
                        "Operator \"||\" requires boolean arguments. First argument is {}.",
                        left.js_string()
                    )));
                };
                if a {
                    return Ok(StyleValue::Boolean(true));
                }
                match right {
                    StyleValue::Boolean(b) => Ok(StyleValue::Boolean(b)),
                    other => Err(RuntimeError::new(format!(
                        "Operator \"||\" requires boolean arguments. Second argument is {}.",
                        other.js_string()
                    ))),
                }
            }
            "=~" | "!~" => {
                let matched = match (&left, &right) {
                    (StyleValue::RegExp(regex), StyleValue::String(input))
                    | (StyleValue::String(input), StyleValue::RegExp(regex)) => regex.borrow_mut().test(input)?,
                    _ => {
                        return Err(RuntimeError::new(format!(
                            "Operator \"{call}\" requires one RegExp argument and one string argument. Arguments are {} and {}.",
                            left.js_string(),
                            right.js_string()
                        )))
                    }
                };
                Ok(StyleValue::Boolean(if call == "!~" { !matched } else { matched }))
            }
            _ if binary_function_names().contains(&call) => evaluate_binary_function(call, left, right),
            _ => Err(RuntimeError::new(format!("Unexpected operator \"{call}\"."))),
        }
    }

    fn evaluate_ternary(&self, feature: Option<&dyn StyleFeature>) -> Result<StyleValue, RuntimeError> {
        let call = self.string_value();
        let left = self.left.as_ref().expect("ternary left").evaluate(feature)?;
        let right = self.right.as_ref().expect("ternary right").evaluate(feature)?;
        let test = self.test.as_ref().expect("ternary test").evaluate(feature)?;
        match call {
            "clamp" => evaluate_ternary_componentwise(call, left, right, test, clamp),
            "mix" => evaluate_ternary_componentwise(call, left, right, test, lerp),
            _ => Err(RuntimeError::new(format!("Unexpected function call \"{call}\"."))),
        }
    }

    fn evaluate_member(&self, feature: Option<&dyn StyleFeature>) -> Result<StyleValue, RuntimeError> {
        if self.left.as_ref().is_some_and(|left| left.is_feature()) {
            let name = self.right.as_ref().expect("member property").evaluate(feature)?;
            return Ok(feature
                .and_then(|feature| feature.property_inherited(&name.js_string()))
                .unwrap_or(StyleValue::Undefined));
        }
        let property = self.left.as_ref().expect("member object").evaluate(feature)?;
        if matches!(property, StyleValue::Undefined | StyleValue::Null) {
            return Ok(StyleValue::Undefined);
        }
        let member = self.right.as_ref().expect("member property").evaluate(feature)?;
        Ok(property.member(&member))
    }
}

fn js_mod(a: f64, b: f64) -> f64 {
    a % b
}

fn evaluate_unary_componentwise(call: &str, value: StyleValue) -> Result<StyleValue, RuntimeError> {
    if call == "length" {
        return match value {
            StyleValue::Number(n) => Ok(StyleValue::Number(n.abs())),
            StyleValue::Cartesian2(v) => Ok(StyleValue::Number(v.magnitude())),
            StyleValue::Cartesian3(v) => Ok(StyleValue::Number(v.magnitude())),
            StyleValue::Cartesian4(v) => Ok(StyleValue::Number(v.magnitude())),
            other => Err(RuntimeError::new(format!(
                "Function \"{call}\" requires a vector or number argument. Argument is {}.",
                other.js_string()
            ))),
        };
    }
    if call == "normalize" {
        return match value {
            StyleValue::Number(_) => Ok(StyleValue::Number(1.0)),
            StyleValue::Cartesian2(v) => Ok(StyleValue::Cartesian2(v.normalize())),
            StyleValue::Cartesian3(v) => Ok(StyleValue::Cartesian3(v.normalize())),
            StyleValue::Cartesian4(v) => Ok(StyleValue::Cartesian4(v.normalize())),
            other => Err(RuntimeError::new(format!(
                "Function \"{call}\" requires a vector or number argument. Argument is {}.",
                other.js_string()
            ))),
        };
    }
    let operation = |x: f64| -> f64 {
        match call {
            "abs" => x.abs(),
            "sqrt" => x.sqrt(),
            "cos" => x.cos(),
            "sin" => x.sin(),
            "tan" => x.tan(),
            "acos" => x.acos(),
            "asin" => x.asin(),
            "atan" => x.atan(),
            "radians" => to_radians(x),
            "degrees" => to_degrees(x),
            "sign" => sign(x),
            "floor" => x.floor(),
            "ceil" => x.ceil(),
            "round" => js_round(x),
            "exp" => x.exp(),
            "exp2" => 2.0f64.powf(x),
            "log" => x.ln(),
            "log2" => log2(x),
            "fract" => x - x.floor(),
            _ => x,
        }
    };
    match value {
        StyleValue::Number(n) => Ok(StyleValue::Number(operation(n))),
        StyleValue::Cartesian2(v) => Ok(StyleValue::Cartesian2(Cartesian2::new(operation(v.x), operation(v.y)))),
        StyleValue::Cartesian3(v) => {
            Ok(StyleValue::Cartesian3(Cartesian3::new(operation(v.x), operation(v.y), operation(v.z))))
        }
        StyleValue::Cartesian4(v) => {
            Ok(StyleValue::Cartesian4(Cartesian4::new(operation(v.x), operation(v.y), operation(v.z), operation(v.w))))
        }
        other => Err(RuntimeError::new(format!(
            "Function \"{call}\" requires a vector or number argument. Argument is {}.",
            other.js_string()
        ))),
    }
}

fn evaluate_binary_function(call: &str, left: StyleValue, right: StyleValue) -> Result<StyleValue, RuntimeError> {
    let componentwise = |operation: &dyn Fn(f64, f64) -> f64| -> Result<StyleValue, RuntimeError> {
        match (&left, &right) {
            (StyleValue::Number(a), StyleValue::Number(b)) => Ok(StyleValue::Number(operation(*a, *b))),
            (StyleValue::Cartesian2(a), StyleValue::Cartesian2(b)) => {
                Ok(StyleValue::Cartesian2(Cartesian2::new(operation(a.x, b.x), operation(a.y, b.y))))
            }
            (StyleValue::Cartesian3(a), StyleValue::Cartesian3(b)) => Ok(StyleValue::Cartesian3(Cartesian3::new(
                operation(a.x, b.x),
                operation(a.y, b.y),
                operation(a.z, b.z),
            ))),
            (StyleValue::Cartesian4(a), StyleValue::Cartesian4(b)) => Ok(StyleValue::Cartesian4(Cartesian4::new(
                operation(a.x, b.x),
                operation(a.y, b.y),
                operation(a.z, b.z),
                operation(a.w, b.w),
            ))),
            _ => Err(RuntimeError::new(format!(
                "Function \"{call}\" requires vector or number arguments of matching types. Arguments are {} and {}.",
                left.js_string(),
                right.js_string()
            ))),
        }
    };
    match call {
        "atan2" => componentwise(&|a, b| a.atan2(b)),
        "pow" => componentwise(&|a, b| a.powf(b)),
        "min" => {
            if let StyleValue::Number(b) = right {
                return match &left {
                    StyleValue::Number(a) => Ok(StyleValue::Number(crate::core::math::js_min(*a, b))),
                    StyleValue::Cartesian2(a) => {
                        Ok(StyleValue::Cartesian2(Cartesian2::new(a.x.min(b), a.y.min(b))))
                    }
                    StyleValue::Cartesian3(a) => {
                        Ok(StyleValue::Cartesian3(Cartesian3::new(a.x.min(b), a.y.min(b), a.z.min(b))))
                    }
                    StyleValue::Cartesian4(a) => Ok(StyleValue::Cartesian4(Cartesian4::new(
                        a.x.min(b),
                        a.y.min(b),
                        a.z.min(b),
                        a.w.min(b),
                    ))),
                    _ => Err(RuntimeError::new(format!(
                        "Function \"{call}\" requires vector or number arguments of matching types. Arguments are {} and {}.",
                        left.js_string(),
                        right.js_string()
                    ))),
                };
            }
            componentwise(&crate::core::math::js_min)
        }
        "max" => {
            if let StyleValue::Number(b) = right {
                return match &left {
                    StyleValue::Number(a) => Ok(StyleValue::Number(crate::core::math::js_max(*a, b))),
                    StyleValue::Cartesian2(a) => {
                        Ok(StyleValue::Cartesian2(Cartesian2::new(a.x.max(b), a.y.max(b))))
                    }
                    StyleValue::Cartesian3(a) => {
                        Ok(StyleValue::Cartesian3(Cartesian3::new(a.x.max(b), a.y.max(b), a.z.max(b))))
                    }
                    StyleValue::Cartesian4(a) => Ok(StyleValue::Cartesian4(Cartesian4::new(
                        a.x.max(b),
                        a.y.max(b),
                        a.z.max(b),
                        a.w.max(b),
                    ))),
                    _ => Err(RuntimeError::new(format!(
                        "Function \"{call}\" requires vector or number arguments of matching types. Arguments are {} and {}.",
                        left.js_string(),
                        right.js_string()
                    ))),
                };
            }
            componentwise(&crate::core::math::js_max)
        }
        "distance" => match (&left, &right) {
            (StyleValue::Number(a), StyleValue::Number(b)) => Ok(StyleValue::Number((a - b).abs())),
            (StyleValue::Cartesian2(a), StyleValue::Cartesian2(b)) => Ok(StyleValue::Number(a.distance(*b))),
            (StyleValue::Cartesian3(a), StyleValue::Cartesian3(b)) => Ok(StyleValue::Number(a.distance(*b))),
            (StyleValue::Cartesian4(a), StyleValue::Cartesian4(b)) => Ok(StyleValue::Number(a.distance(*b))),
            _ => Err(RuntimeError::new(format!(
                "Function \"{call}\" requires vector or number arguments of matching types. Arguments are {} and {}.",
                left.js_string(),
                right.js_string()
            ))),
        },
        "dot" => match (&left, &right) {
            (StyleValue::Number(a), StyleValue::Number(b)) => Ok(StyleValue::Number(a * b)),
            (StyleValue::Cartesian2(a), StyleValue::Cartesian2(b)) => Ok(StyleValue::Number(a.dot(*b))),
            (StyleValue::Cartesian3(a), StyleValue::Cartesian3(b)) => Ok(StyleValue::Number(a.dot(*b))),
            (StyleValue::Cartesian4(a), StyleValue::Cartesian4(b)) => Ok(StyleValue::Number(a.dot(*b))),
            _ => Err(RuntimeError::new(format!(
                "Function \"{call}\" requires vector or number arguments of matching types. Arguments are {} and {}.",
                left.js_string(),
                right.js_string()
            ))),
        },
        "cross" => match (&left, &right) {
            (StyleValue::Cartesian3(a), StyleValue::Cartesian3(b)) => Ok(StyleValue::Cartesian3(a.cross(*b))),
            _ => Err(RuntimeError::new(format!(
                "Function \"{call}\" requires vec3 arguments. Arguments are {} and {}.",
                left.js_string(),
                right.js_string()
            ))),
        },
        _ => Err(RuntimeError::new(format!("Unexpected function call \"{call}\"."))),
    }
}

fn evaluate_ternary_componentwise(
    call: &str,
    left: StyleValue,
    right: StyleValue,
    test: StyleValue,
    operation: impl Fn(f64, f64, f64) -> f64,
) -> Result<StyleValue, RuntimeError> {
    if let StyleValue::Number(t) = test {
        return match (&left, &right) {
            (StyleValue::Number(a), StyleValue::Number(b)) => Ok(StyleValue::Number(operation(*a, *b, t))),
            (StyleValue::Cartesian2(a), StyleValue::Cartesian2(b)) => Ok(StyleValue::Cartesian2(Cartesian2::new(
                operation(a.x, b.x, t),
                operation(a.y, b.y, t),
            ))),
            (StyleValue::Cartesian3(a), StyleValue::Cartesian3(b)) => Ok(StyleValue::Cartesian3(Cartesian3::new(
                operation(a.x, b.x, t),
                operation(a.y, b.y, t),
                operation(a.z, b.z, t),
            ))),
            (StyleValue::Cartesian4(a), StyleValue::Cartesian4(b)) => Ok(StyleValue::Cartesian4(Cartesian4::new(
                operation(a.x, b.x, t),
                operation(a.y, b.y, t),
                operation(a.z, b.z, t),
                operation(a.w, b.w, t),
            ))),
            _ => Err(RuntimeError::new(format!(
                "Function \"{call}\" requires vector or number arguments of matching types. Arguments are {}, {}, and {}.",
                left.js_string(),
                right.js_string(),
                test.js_string()
            ))),
        };
    }
    match (&left, &right, &test) {
        (StyleValue::Number(a), StyleValue::Number(b), StyleValue::Number(t)) => {
            Ok(StyleValue::Number(operation(*a, *b, *t)))
        }
        (StyleValue::Cartesian2(a), StyleValue::Cartesian2(b), StyleValue::Cartesian2(t)) => {
            Ok(StyleValue::Cartesian2(Cartesian2::new(operation(a.x, b.x, t.x), operation(a.y, b.y, t.y))))
        }
        (StyleValue::Cartesian3(a), StyleValue::Cartesian3(b), StyleValue::Cartesian3(t)) => {
            Ok(StyleValue::Cartesian3(Cartesian3::new(
                operation(a.x, b.x, t.x),
                operation(a.y, b.y, t.y),
                operation(a.z, b.z, t.z),
            )))
        }
        (StyleValue::Cartesian4(a), StyleValue::Cartesian4(b), StyleValue::Cartesian4(t)) => {
            Ok(StyleValue::Cartesian4(Cartesian4::new(
                operation(a.x, b.x, t.x),
                operation(a.y, b.y, t.y),
                operation(a.z, b.z, t.z),
                operation(a.w, b.w, t.w),
            )))
        }
        _ => Err(RuntimeError::new(format!(
            "Function \"{call}\" requires vector or number arguments of matching types. Arguments are {}, {}, and {}.",
            left.js_string(),
            right.js_string(),
            test.js_string()
        ))),
    }
}

// ---------------------------------------------------------------------------
// Shader generation
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct ShaderState {
    pub translucent: bool,
}

const NULL_SENTINEL: &str = "czm_infinity";

impl Node {
    fn shader_expression(
        &self,
        map: &BTreeMap<String, String>,
        state: &mut ShaderState,
        parent: Option<&Node>,
    ) -> Result<Option<String>, RuntimeError> {
        use NodeKind::*;
        let left = match &self.left {
            Some(left) => left.shader_expression(map, state, Some(self))?,
            None => None,
        };
        let right = match &self.right {
            Some(right) => right.shader_expression(map, state, Some(self))?,
            None => None,
        };
        let test = match &self.test {
            Some(test) => test.shader_expression(map, state, Some(self))?,
            None => None,
        };
        let mut children = Vec::new();
        if self.kind == NodeKind::LiteralColor || self.kind == NodeKind::LiteralVector || self.kind == NodeKind::Array {
            for child in self.children() {
                children.push(child.shader_expression(map, state, Some(self))?);
            }
        }
        let value = self.string_value();
        let need = |option: &Option<String>| option.clone().unwrap_or_default();

        match self.kind {
            Variable => {
                if self.is_feature() {
                    return Ok(None);
                }
                Ok(Some(map.get(value).cloned().unwrap_or_else(|| NULL_SENTINEL.to_string())))
            }
            Unary => {
                if value == "Boolean" {
                    return Ok(Some(format!("bool({})", need(&left))));
                } else if value == "Number" {
                    return Ok(Some(format!("float({})", need(&left))));
                } else if value == "round" {
                    return Ok(Some(format!("floor({} + 0.5)", need(&left))));
                } else if unary_function_names().contains(&value) {
                    return Ok(Some(format!("{value}({})", need(&left))));
                } else if value == "isNaN" {
                    let left = need(&left);
                    return Ok(Some(format!("({left} != {left})")));
                } else if value == "isFinite" {
                    return Ok(Some(format!("(abs({}) < czm_infinity)", need(&left))));
                } else if matches!(value, "String" | "isExactClass" | "isClass" | "getExactClassName") {
                    return Err(RuntimeError::new(format!(
                        "Error generating style shader: \"{value}\" is not supported."
                    )));
                }
                Ok(Some(format!("{value}{}", need(&left))))
            }
            Binary => {
                if value == "%" {
                    return Ok(Some(format!("mod({}, {})", need(&left), need(&right))));
                } else if value == "===" {
                    return Ok(Some(format!("({} == {})", need(&left), need(&right))));
                } else if value == "!==" {
                    return Ok(Some(format!("({} != {})", need(&left), need(&right))));
                } else if value == "atan2" {
                    return Ok(Some(format!("atan({}, {})", need(&left), need(&right))));
                } else if binary_function_names().contains(&value) {
                    return Ok(Some(format!("{value}({}, {})", need(&left), need(&right))));
                }
                Ok(Some(format!("({} {value} {})", need(&left), need(&right))))
            }
            Ternary => {
                if ternary_function_names().contains(&value) {
                    return Ok(Some(format!("{value}({}, {}, {})", need(&left), need(&right), need(&test))));
                }
                Ok(None)
            }
            Conditional => Ok(Some(format!("({} ? {} : {})", need(&test), need(&left), need(&right)))),
            Member => {
                if self.left.as_ref().is_some_and(|left| left.is_feature()) {
                    let prop = right.as_deref().unwrap_or_default();
                    return Ok(Some(map.get(prop).cloned().unwrap_or_else(|| NULL_SENTINEL.to_string())));
                }
                let member = right.clone().unwrap_or_default();
                if matches!(member.as_str(), "r" | "x" | "0.0") {
                    return Ok(Some(format!("{}[0]", need(&left))));
                } else if matches!(member.as_str(), "g" | "y" | "1.0") {
                    return Ok(Some(format!("{}[1]", need(&left))));
                } else if matches!(member.as_str(), "b" | "z" | "2.0") {
                    return Ok(Some(format!("{}[2]", need(&left))));
                } else if matches!(member.as_str(), "a" | "w" | "3.0") {
                    return Ok(Some(format!("{}[3]", need(&left))));
                }
                Ok(Some(format!("{}[int({member})]", need(&left))))
            }
            FunctionCall => {
                Err(RuntimeError::new(format!("Error generating style shader: \"{value}\" is not supported.")))
            }
            Array => {
                if children.len() == 4 {
                    Ok(Some(format!(
                        "vec4({}, {}, {}, {})",
                        need(&children[0]),
                        need(&children[1]),
                        need(&children[2]),
                        need(&children[3])
                    )))
                } else if children.len() == 3 {
                    Ok(Some(format!("vec3({}, {}, {})", need(&children[0]), need(&children[1]), need(&children[2]))))
                } else if children.len() == 2 {
                    Ok(Some(format!("vec2({}, {})", need(&children[0]), need(&children[1]))))
                } else {
                    Err(RuntimeError::new(
                        "Error generating style shader: Invalid array length. Array length should be 2, 3, or 4."
                            .to_string(),
                    ))
                }
            }
            Regex => Err(RuntimeError::new(
                "Error generating style shader: Regular expressions are not supported.".to_string(),
            )),
            VariableInString => Err(RuntimeError::new(
                "Error generating style shader: Converting a variable to a string is not supported.".to_string(),
            )),
            LiteralNull | LiteralUndefined => Ok(Some(NULL_SENTINEL.to_string())),
            LiteralBoolean => {
                Ok(Some(if matches!(self.value, NodeValue::Boolean(true)) { "true".into() } else { "false".into() }))
            }
            LiteralNumber => {
                let NodeValue::Number(number) = self.value else {
                    return Ok(None);
                };
                Ok(Some(number_to_string(number)))
            }
            LiteralString => {
                if let Some(parent) = parent {
                    if parent.kind == NodeKind::Member
                        && (matches!(value, "r" | "g" | "b" | "a" | "x" | "y" | "z" | "w")
                            || parent.left.as_ref().is_some_and(|left| left.is_feature()))
                    {
                        return Ok(Some(value.to_string()));
                    }
                }
                if let Some(color) = Color::from_css_color_string(value) {
                    return Ok(Some(color_to_vec3(&color)));
                }
                Err(RuntimeError::new("Error generating style shader: String literals are not supported.".to_string()))
            }
            LiteralColor => {
                if value == "color" {
                    if children.is_empty() {
                        return Ok(Some("vec4(1.0)".into()));
                    } else if children.len() > 1 {
                        let rgb = children[0].clone().unwrap_or_default();
                        let alpha = children[1].clone().unwrap_or_default();
                        if alpha != "1.0" {
                            state.translucent = true;
                        }
                        return Ok(Some(format!("vec4({rgb}, {alpha})")));
                    }
                    return Ok(Some(format!("vec4({}, 1.0)", need(&children[0]))));
                } else if value == "rgb" {
                    if let Some(color) = convert_rgb_to_color(self) {
                        return Ok(Some(color_to_vec4(&color)));
                    }
                    return Ok(Some(format!(
                        "vec4({} / 255.0, {} / 255.0, {} / 255.0, 1.0)",
                        need(&children[0]),
                        need(&children[1]),
                        need(&children[2])
                    )));
                } else if value == "rgba" {
                    if children.get(3).is_some_and(|alpha| alpha.as_deref() != Some("1.0")) {
                        state.translucent = true;
                    }
                    if let Some(color) = convert_rgb_to_color(self) {
                        return Ok(Some(color_to_vec4(&color)));
                    }
                    return Ok(Some(format!(
                        "vec4({} / 255.0, {} / 255.0, {} / 255.0, {})",
                        need(&children[0]),
                        need(&children[1]),
                        need(&children[2]),
                        need(&children[3])
                    )));
                } else if value == "hsl" {
                    if let Some(color) = convert_hsl_to_rgb(self) {
                        return Ok(Some(color_to_vec4(&color)));
                    }
                    return Ok(Some(format!(
                        "vec4(czm_HSLToRGB(vec3({}, {}, {})), 1.0)",
                        need(&children[0]),
                        need(&children[1]),
                        need(&children[2])
                    )));
                } else if value == "hsla" {
                    if let Some(color) = convert_hsl_to_rgb(self) {
                        if color.alpha != 1.0 {
                            state.translucent = true;
                        }
                        return Ok(Some(color_to_vec4(&color)));
                    }
                    if children.get(3).is_some_and(|alpha| alpha.as_deref() != Some("1.0")) {
                        state.translucent = true;
                    }
                    return Ok(Some(format!(
                        "vec4(czm_HSLToRGB(vec3({}, {}, {})), {})",
                        need(&children[0]),
                        need(&children[1]),
                        need(&children[2]),
                        need(&children[3])
                    )));
                }
                Err(RuntimeError::new(format!("Error generating style shader: \"{value}\" is not supported.")))
            }
            LiteralVector => {
                let mut expression = format!("{value}(");
                for (i, component) in children.iter().enumerate() {
                    expression.push_str(&need(component));
                    if i < children.len() - 1 {
                        expression.push_str(", ");
                    }
                }
                expression.push(')');
                Ok(Some(expression))
            }
            LiteralRegex => Err(RuntimeError::new(
                "Error generating style shader: Regular expressions are not supported.".to_string(),
            )),
            BuiltinVariable => {
                if value == "tiles3d_tileset_time" {
                    Ok(Some(value.to_string()))
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn collect_variables(&self, variables: &mut Vec<String>, parent: Option<&Node>) {
        if let Some(left) = &self.left {
            left.collect_variables(variables, Some(self));
        }
        if let Some(right) = &self.right {
            right.collect_variables(variables, Some(self));
        }
        if let Some(test) = &self.test {
            test.collect_variables(variables, Some(self));
        }
        for child in self.children() {
            child.collect_variables(variables, Some(self));
        }
        match self.kind {
            NodeKind::Variable => {
                if !self.is_feature() {
                    variables.push(self.string_value().to_string());
                }
            }
            NodeKind::VariableInString => {
                let text = self.string_value();
                let mut rest = text;
                while let Some(start) = rest.find("${") {
                    let Some(close) = rest[start..].find('}').map(|c| c + start) else {
                        break;
                    };
                    variables.push(rest[start + 2..close].to_string());
                    rest = &rest[close + 1..];
                }
            }
            NodeKind::LiteralString => {
                if let Some(parent) = parent {
                    if parent.kind == NodeKind::Member && parent.left.as_ref().is_some_and(|left| left.is_feature()) {
                        variables.push(self.string_value().to_string());
                    }
                }
            }
            _ => {}
        }
    }
}

fn number_to_string(number: f64) -> String {
    if number.fract() == 0.0 {
        js_to_fixed(number, 1)
    } else {
        js_number_to_string(number)
    }
}

fn color_to_vec3(color: &Color) -> String {
    format!(
        "vec3({}, {}, {})",
        number_to_string(color.red),
        number_to_string(color.green),
        number_to_string(color.blue)
    )
}

fn color_to_vec4(color: &Color) -> String {
    format!(
        "vec4({}, {}, {}, {})",
        number_to_string(color.red),
        number_to_string(color.green),
        number_to_string(color.blue),
        number_to_string(color.alpha)
    )
}

fn convert_rgb_to_color(node: &Node) -> Option<Color> {
    let channels = node.children();
    for channel in channels {
        if channel.kind != NodeKind::LiteralNumber {
            return None;
        }
    }
    let number = |index: usize| match channels.get(index).map(|c| &c.value) {
        Some(NodeValue::Number(n)) => *n,
        _ => 0.0,
    };
    Some(Color::new(
        number(0) / 255.0,
        number(1) / 255.0,
        number(2) / 255.0,
        if channels.len() == 4 { number(3) } else { 1.0 },
    ))
}

fn convert_hsl_to_rgb(node: &Node) -> Option<Color> {
    let channels = node.children();
    for channel in channels {
        if channel.kind != NodeKind::LiteralNumber {
            return None;
        }
    }
    let number = |index: usize| match channels.get(index).map(|c| &c.value) {
        Some(NodeValue::Number(n)) => *n,
        _ => 0.0,
    };
    Some(Color::from_hsl(number(0), number(1), number(2), if channels.len() == 4 { number(3) } else { 1.0 }))
}

// ---------------------------------------------------------------------------
// Expression
// ---------------------------------------------------------------------------

/// CesiumJS `Expression`.
#[derive(Clone)]
pub struct Expression {
    expression: String,
    runtime_ast: Rc<Node>,
}

impl Expression {
    pub fn new(expression: &str, defines: Option<&BTreeMap<String, String>>) -> Result<Self, RuntimeError> {
        let replaced = replace_defines(expression, defines);
        let replaced = replace_variables(&remove_backslashes(&replaced))?;
        let ast = Parser::new(&replaced)?.parse()?;
        let runtime_ast = create_runtime_ast(&ast)?;
        Ok(Self { expression: expression.to_string(), runtime_ast: Rc::new(runtime_ast) })
    }

    pub fn expression(&self) -> &str {
        &self.expression
    }

    pub fn evaluate(&self, feature: Option<&dyn StyleFeature>) -> Result<StyleValue, RuntimeError> {
        self.runtime_ast.evaluate(feature)
    }

    /// Evaluates the expression as a color, as `Color.fromCartesian4` does.
    pub fn evaluate_color(&self, feature: Option<&dyn StyleFeature>) -> Result<Option<Color>, RuntimeError> {
        match self.evaluate(feature)? {
            StyleValue::Undefined => Ok(None),
            StyleValue::Cartesian4(c) => Ok(Some(Color::from_cartesian4(c))),
            other => Err(RuntimeError::new(format!(
                "Expected a Color expression; the expression evaluated to {}.",
                other.js_string()
            ))),
        }
    }

    pub fn get_shader_expression(
        &self,
        variable_substitution_map: &BTreeMap<String, String>,
        shader_state: &mut ShaderState,
    ) -> Result<Option<String>, RuntimeError> {
        self.runtime_ast.shader_expression(variable_substitution_map, shader_state, None)
    }

    pub fn get_shader_function(
        &self,
        function_signature: &str,
        variable_substitution_map: &BTreeMap<String, String>,
        shader_state: &mut ShaderState,
        return_type: &str,
    ) -> Result<String, RuntimeError> {
        let expression = self
            .get_shader_expression(variable_substitution_map, shader_state)?
            .unwrap_or_else(|| "undefined".to_string());
        Ok(format!("{return_type} {function_signature}\n{{\n    return {expression};\n}}\n"))
    }

    pub fn get_variables(&self) -> Vec<String> {
        let mut variables = Vec::new();
        self.runtime_ast.collect_variables(&mut variables, None);
        dedupe(variables)
    }
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

impl fmt::Debug for Expression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Expression").field("expression", &self.expression).finish()
    }
}

impl PartialEq for Expression {
    fn eq(&self, other: &Self) -> bool {
        self.expression == other.expression
    }
}
