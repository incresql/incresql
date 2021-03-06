use data::rust_decimal::Decimal;
use data::{DataType, Datum, SortOrder};
use functions::{AggregateFunction, Function, FunctionSignature};
use regex::Regex;
use std::cmp::max;
use std::fmt::{Display, Formatter};
use std::iter::{empty, once};

/// The expression ast.
/// For scalar expressions we support evaluating the ast directly,
/// but for aggregate expressions you'll first need to transform
/// into an AggregateExpression (from the executor crate).
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Expression {
    Constant(Datum<'static>, DataType),
    FunctionCall(FunctionCall),
    Cast(Cast),
    CompiledFunctionCall(CompiledFunctionCall),
    CompiledAggregate(CompiledAggregate),
    ColumnReference(ColumnReference),
    CompiledColumnReference(CompiledColumnReference),
}

impl Default for Expression {
    fn default() -> Self {
        Expression::Constant(Datum::Null, DataType::Null)
    }
}

/// Represents a function call straight from the parser.
/// Ie the function isn't actually resolved by this point
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct FunctionCall {
    pub function_name: String,
    pub args: Vec<Expression>,
}

/// Represents a sql cast, gets compiled to a function
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Cast {
    pub expr: Box<Expression>,
    pub datatype: DataType,
}

/// Represents a scalar function call once its been resolved and type
/// checked
#[derive(Debug, Clone)]
pub struct CompiledFunctionCall {
    // This is a bit overweight(7 words) and is blowing out the size of the Expression
    // enum a bit hence the boxed slices instead of vec's
    pub function: &'static dyn Function,
    pub args: Box<[Expression]>,
    // Used to store the evaluation results of the sub expressions during execution
    pub expr_buffer: Box<[Datum<'static>]>,
    pub signature: Box<FunctionSignature<'static>>,
}

impl PartialEq for CompiledFunctionCall {
    fn eq(&self, other: &Self) -> bool {
        self.args == other.args && self.signature == other.signature
    }
}

impl Eq for CompiledFunctionCall {}

/// Represents a aggregate function call once its been resolved and type
/// checked
#[derive(Debug, Clone)]
pub struct CompiledAggregate {
    pub function: &'static dyn AggregateFunction,
    pub args: Box<[Expression]>,
    // Used to store the evaluation results of the sub expressions during execution
    pub expr_buffer: Box<[Datum<'static>]>,
    pub signature: Box<FunctionSignature<'static>>,
}

impl PartialEq for CompiledAggregate {
    fn eq(&self, other: &Self) -> bool {
        self.args == other.args && self.signature == other.signature
    }
}

impl Eq for CompiledAggregate {}

/// A reference to a column in a source.
/// ie SELECT foo FROM...
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ColumnReference {
    pub qualifier: Option<String>,
    pub alias: String,
    // Easier to just make this a bool rather than pattern match on
    // an enum, an alias of "*" isn't enough as that *could* be a valid
    // alias... (who on earth would do that tho!).
    pub star: bool,
}

/// Column reference but is indexed via offset instead of having to do
/// name resolution...
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct CompiledColumnReference {
    pub offset: usize,
    pub datatype: DataType,
}

/// Named expression, ie select foo as bar
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct NamedExpression {
    pub alias: Option<String>,
    pub expression: Expression,
}

/// Sort expression, ie order by abs(foo) desc
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SortExpression {
    pub ordering: SortOrder,
    pub expression: Expression,
}

impl Expression {
    // Iterates over all child expressions.
    pub fn children(&self) -> Box<dyn Iterator<Item = &Expression> + '_> {
        match self {
            Expression::FunctionCall(function_call) => Box::from(function_call.args.iter()),
            Expression::CompiledFunctionCall(function_call) => Box::from(function_call.args.iter()),
            Expression::CompiledAggregate(function_call) => Box::from(function_call.args.iter()),
            Expression::Cast(cast) => Box::from(once(&*cast.expr)),
            Expression::CompiledColumnReference(_)
            | Expression::Constant(_, _)
            | Expression::ColumnReference(_) => Box::from(empty()),
        }
    }

    // Iterates over all child expressions.
    pub fn children_mut(&mut self) -> Box<dyn Iterator<Item = &mut Expression> + '_> {
        match self {
            Expression::FunctionCall(function_call) => Box::from(function_call.args.iter_mut()),
            Expression::CompiledFunctionCall(function_call) => {
                Box::from(function_call.args.iter_mut())
            }
            Expression::CompiledAggregate(function_call) => {
                Box::from(function_call.args.iter_mut())
            }
            Expression::Cast(cast) => Box::from(once(&mut *cast.expr)),
            Expression::CompiledColumnReference(_)
            | Expression::Constant(_, _)
            | Expression::ColumnReference(_) => Box::from(empty()),
        }
    }
}

// Convenience helpers to construct expression literals
impl From<bool> for Expression {
    fn from(b: bool) -> Self {
        Expression::Constant(Datum::from(b), DataType::Boolean)
    }
}

impl From<i32> for Expression {
    fn from(i: i32) -> Self {
        Expression::Constant(Datum::from(i), DataType::Integer)
    }
}

impl From<i64> for Expression {
    fn from(i: i64) -> Self {
        Expression::Constant(Datum::from(i), DataType::BigInt)
    }
}

impl From<Decimal> for Expression {
    fn from(d: Decimal) -> Self {
        let s = d.scale() as u8;
        // A bit yuk, there's no integer log10 yet
        let mut p = 0;
        let mut temp = d.abs().trunc();
        while temp != Decimal::new(0, 0) {
            p += 1;
            temp /= Decimal::new(10, 0);
            temp = temp.trunc();
        }
        p = max(p + s, 1);
        Expression::Constant(Datum::from(d), DataType::Decimal(p, s))
    }
}

impl From<&'static str> for Expression {
    fn from(s: &'static str) -> Self {
        Expression::Constant(Datum::from(s), DataType::Text)
    }
}

impl From<String> for Expression {
    fn from(s: String) -> Self {
        Expression::Constant(Datum::from(s), DataType::Text)
    }
}

lazy_static! {
    /// If we an identifier matches this then we don't need to quote it
    static ref IDENTIFIER_OK: Regex = Regex::new(r"^([a-z]|_)([a-z,0-9]|_)*$").unwrap();
}

impl Display for Expression {
    /// Formats the expression back to sql
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Expression::Constant(d, dt) => f.write_fmt(format_args!("{:#}", d.typed_with(*dt))),
            Expression::Cast(c) => f.write_fmt(format_args!("CAST({} AS {})", c.expr, c.datatype)),
            // For any function name containing anything other that letters and underscores we'll quote.
            Expression::FunctionCall(function_call) => {
                let args = function_call
                    .args
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                if IDENTIFIER_OK.is_match(&function_call.function_name) {
                    f.write_fmt(format_args!("{}({})", function_call.function_name, args))
                } else {
                    f.write_fmt(format_args!("`{}`({})", function_call.function_name, args))
                }
            }
            Expression::CompiledFunctionCall(function_call) => {
                let args = function_call
                    .args
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                if IDENTIFIER_OK.is_match(&function_call.signature.name) {
                    f.write_fmt(format_args!("{}({})", function_call.signature.name, args))
                } else {
                    f.write_fmt(format_args!("`{}`({})", function_call.signature.name, args))
                }
            }
            Expression::CompiledAggregate(function_call) => {
                let args = function_call
                    .args
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                if IDENTIFIER_OK.is_match(&function_call.signature.name) {
                    f.write_fmt(format_args!("{}({})", function_call.signature.name, args))
                } else {
                    f.write_fmt(format_args!("`{}`({})", function_call.signature.name, args))
                }
            }
            Expression::ColumnReference(column_reference) => Display::fmt(column_reference, f),
            Expression::CompiledColumnReference(column_reference) => {
                // To turn this back into real sql we would need to be able to have a peek at
                // our sources
                f.write_fmt(format_args!("<OFFSET {}>", &column_reference.offset))
            }
        }
    }
}

impl Display for NamedExpression {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(alias) = &self.alias {
            if IDENTIFIER_OK.is_match(alias) {
                f.write_fmt(format_args!("{} AS {}", self.expression, alias))
            } else {
                f.write_fmt(format_args!("{} AS `{}`", self.expression, alias))
            }
        } else {
            Display::fmt(&self.expression, f)
        }
    }
}

impl Display for ColumnReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(qualifier) = &self.qualifier {
            if IDENTIFIER_OK.is_match(qualifier) {
                f.write_fmt(format_args!("{}.", qualifier))?;
            } else {
                f.write_fmt(format_args!("`{}`.", qualifier))?;
            }
        }

        if IDENTIFIER_OK.is_match(&self.alias) {
            f.write_fmt(format_args!("{}", &self.alias))
        } else {
            f.write_fmt(format_args!("`{}`", &self.alias))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expr_size() {
        // This is already way larger than I would have liked...
        assert_eq!(std::mem::size_of::<Expression>(), 64);
    }

    #[test]
    fn test_expr_from_boolean() {
        assert_eq!(
            Expression::from(true),
            Expression::Constant(Datum::Boolean(true), DataType::Boolean)
        );
        assert_eq!(
            Expression::from(false),
            Expression::Constant(Datum::Boolean(false), DataType::Boolean)
        );
    }

    #[test]
    fn test_expr_from_integer() {
        assert_eq!(
            Expression::from(1234),
            Expression::Constant(Datum::Integer(1234), DataType::Integer)
        );
    }

    #[test]
    fn test_expr_from_bigint() {
        assert_eq!(
            Expression::from(1234_i64),
            Expression::Constant(Datum::BigInt(1234), DataType::BigInt)
        );
    }

    #[test]
    fn test_expr_from_decimal() {
        assert_eq!(
            Expression::from(Decimal::new(12345, 2)),
            Expression::Constant(
                Datum::Decimal(Decimal::new(12345, 2)),
                DataType::Decimal(5, 2)
            )
        );

        assert_eq!(
            Expression::from(Decimal::new(-12345, 2)),
            Expression::Constant(
                Datum::Decimal(Decimal::new(-12345, 2)),
                DataType::Decimal(5, 2)
            )
        );

        assert_eq!(
            Expression::from(Decimal::new(0, 1)),
            Expression::Constant(Datum::Decimal(Decimal::new(0, 1)), DataType::Decimal(1, 1))
        );

        assert_eq!(
            Expression::from(Decimal::new(1234, 4)),
            Expression::Constant(
                Datum::Decimal(Decimal::new(1234, 4)),
                DataType::Decimal(4, 4)
            )
        );
    }

    #[test]
    fn test_expr_from_string() {
        assert_eq!(
            Expression::from(String::from("Hello world")),
            Expression::Constant(
                Datum::ByteAOwned(Box::from(b"Hello world".as_ref())),
                DataType::Text
            )
        );
    }

    #[test]
    fn test_expr_to_string() {
        let expr = Expression::FunctionCall(FunctionCall {
            function_name: "+".to_string(),
            args: vec![
                Expression::Cast(Cast {
                    expr: Box::new(Expression::from("5")),
                    datatype: DataType::Integer,
                }),
                Expression::FunctionCall(FunctionCall {
                    function_name: "pow".to_string(),
                    args: vec![Expression::from(Decimal::new(23, 1)), Expression::from(2)],
                }),
            ],
        });

        assert_eq!(
            expr.to_string(),
            r#"`+`(CAST("5" AS INTEGER), pow(2.3, 2))"#
        );
    }

    #[test]
    fn test_named_expr_to_string() {
        let expr = NamedExpression {
            alias: None,
            expression: Expression::from(1),
        };

        assert_eq!(expr.to_string(), "1");

        let expr = NamedExpression {
            alias: Some(String::from("foo")),
            expression: Expression::from(1),
        };

        assert_eq!(expr.to_string(), "1 AS foo");

        let expr = NamedExpression {
            alias: Some(String::from("1b")),
            expression: Expression::from(1),
        };

        assert_eq!(expr.to_string(), "1 AS `1b`");
    }
}
