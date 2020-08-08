use crate::registry::Registry;
use crate::{Function, FunctionDefinition};
use data::{DataType, Datum, Session, DECIMAL_MAX_PRECISION, DECIMAL_MAX_SCALE};
use std::cmp::min;

#[derive(Debug)]
struct MultiplyInteger {}

impl Function for MultiplyInteger {
    fn execute<'a>(&self, _session: &Session, args: &'a [Datum<'a>]) -> Datum<'a> {
        if let (Some(a), Some(b)) = (args[0].as_integer(), args[1].as_integer()) {
            Datum::from(a * b)
        } else {
            Datum::Null
        }
    }
}

#[derive(Debug)]
struct MultiplyBigint {}

impl Function for MultiplyBigint {
    fn execute<'a>(&self, _session: &Session, args: &'a [Datum<'a>]) -> Datum<'a> {
        if let (Some(a), Some(b)) = (args[0].as_bigint(), args[1].as_bigint()) {
            Datum::from(a * b)
        } else {
            Datum::Null
        }
    }
}

#[derive(Debug)]
struct MultiplyDecimal {}

impl Function for MultiplyDecimal {
    fn execute<'a>(&self, _session: &Session, args: &'a [Datum<'a>]) -> Datum<'a> {
        if let (Some(a), Some(b)) = (args[0].as_decimal(), args[1].as_decimal()) {
            let mut d = a * b;
            // Rescale to ensure we stay matching what the sql types say
            if d.scale() > DECIMAL_MAX_SCALE as u32 {
                d.rescale(DECIMAL_MAX_SCALE as u32);
            }
            Datum::from(d)
        } else {
            Datum::Null
        }
    }
}

pub fn register_builtins(registry: &mut Registry) {
    registry.register_function(FunctionDefinition::new(
        "*",
        vec![DataType::Integer, DataType::Integer],
        DataType::Integer,
        &MultiplyInteger {},
    ));

    registry.register_function(FunctionDefinition::new(
        "*",
        vec![DataType::BigInt, DataType::BigInt],
        DataType::BigInt,
        &MultiplyBigint {},
    ));

    registry.register_function(FunctionDefinition::new_with_type_resolver(
        "*",
        vec![DataType::Decimal(0, 0), DataType::Decimal(0, 0)],
        |args| {
            if let (DataType::Decimal(p1, s1), DataType::Decimal(p2, s2)) = (args[0], args[1]) {
                DataType::Decimal(
                    min(p1 + p2, DECIMAL_MAX_PRECISION),
                    min(s1 + s2, DECIMAL_MAX_SCALE),
                )
            } else {
                panic!()
            }
        },
        &MultiplyDecimal {},
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use data::Decimal;

    #[test]
    fn test_null() {
        assert_eq!(
            MultiplyInteger {}.execute(&Session::new(1), &[Datum::Null, Datum::Null]),
            Datum::Null
        )
    }

    #[test]
    fn test_add_int() {
        assert_eq!(
            MultiplyInteger {}.execute(&Session::new(1), &[Datum::from(3), Datum::from(2)]),
            Datum::from(6)
        )
    }

    #[test]
    fn test_add_bigint() {
        assert_eq!(
            MultiplyBigint {}.execute(&Session::new(1), &[Datum::from(3_i64), Datum::from(2_i64)]),
            Datum::from(6_i64)
        )
    }

    #[test]
    fn test_add_decimal() {
        assert_eq!(
            MultiplyDecimal {}.execute(
                &Session::new(1),
                &[
                    Datum::from(Decimal::new(30, 1)),
                    Datum::from(Decimal::new(200, 2))
                ]
            ),
            Datum::from(Decimal::new(6000, 3))
        )
    }
}