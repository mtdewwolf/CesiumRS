//! CesiumJS `ExpressionNodeType`: node kinds in a parsed 3D Tiles style expression.

numeric_enum!(ExpressionNodeType: i32 {
    Variable = 0 => "VARIABLE",
    Unary = 1 => "UNARY",
    Binary = 2 => "BINARY",
    Ternary = 3 => "TERNARY",
    Conditional = 4 => "CONDITIONAL",
    Member = 5 => "MEMBER",
    FunctionCall = 6 => "FUNCTION_CALL",
    Array = 7 => "ARRAY",
    Regex = 8 => "REGEX",
    VariableInString = 9 => "VARIABLE_IN_STRING",
    LiteralNull = 10 => "LITERAL_NULL",
    LiteralBoolean = 11 => "LITERAL_BOOLEAN",
    LiteralNumber = 12 => "LITERAL_NUMBER",
    LiteralString = 13 => "LITERAL_STRING",
    LiteralColor = 14 => "LITERAL_COLOR",
    LiteralVector = 15 => "LITERAL_VECTOR",
    LiteralRegex = 16 => "LITERAL_REGEX",
    LiteralUndefined = 17 => "LITERAL_UNDEFINED",
    BuiltinVariable = 18 => "BUILTIN_VARIABLE",
});
