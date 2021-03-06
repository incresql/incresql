use crate::registry::Registry;
use crate::{Function, FunctionDefinition, FunctionSignature, FunctionType};
use data::rust_decimal::prelude::ToPrimitive;
use data::{DataType, Datum, Session};

#[derive(Debug)]
struct ToIntFromBoolean {}

impl Function for ToIntFromBoolean {
    fn execute<'a>(
        &self,
        _session: &Session,
        _signature: &FunctionSignature,
        args: &'a [Datum<'a>],
    ) -> Datum<'a> {
        if let Some(a) = args[0].as_maybe_boolean() {
            Datum::Integer(if a { 1 } else { 0 })
        } else {
            Datum::Null
        }
    }
}

#[derive(Debug)]
struct ToIntFromInt {}

impl Function for ToIntFromInt {
    fn execute<'a>(
        &self,
        _session: &Session,
        _signature: &FunctionSignature,
        args: &'a [Datum<'a>],
    ) -> Datum<'a> {
        args[0].clone()
    }
}

#[derive(Debug)]
struct ToIntFromBigInt {}

impl Function for ToIntFromBigInt {
    fn execute<'a>(
        &self,
        _session: &Session,
        _signature: &FunctionSignature,
        args: &'a [Datum<'a>],
    ) -> Datum<'a> {
        if let Some(a) = args[0].as_maybe_bigint() {
            Datum::from(a as i32)
        } else {
            Datum::Null
        }
    }
}

#[derive(Debug)]
struct ToIntFromDecimal {}

impl Function for ToIntFromDecimal {
    fn execute<'a>(
        &self,
        _session: &Session,
        _signature: &FunctionSignature,
        args: &'a [Datum<'a>],
    ) -> Datum<'a> {
        if let Some(a) = args[0].as_maybe_decimal() {
            a.to_i32().map(Datum::from).unwrap_or(Datum::Null)
        } else {
            Datum::Null
        }
    }
}

#[derive(Debug)]
struct ToIntFromText {}

impl Function for ToIntFromText {
    fn execute<'a>(
        &self,
        _session: &Session,
        _signature: &FunctionSignature,
        args: &'a [Datum<'a>],
    ) -> Datum<'a> {
        if let Some(a) = args[0].as_maybe_text() {
            a.parse::<i32>()
                .ok()
                .map(Datum::from)
                .unwrap_or(Datum::Null)
        } else {
            Datum::Null
        }
    }
}

#[derive(Debug)]
struct ToIntFromJson {}

impl Function for ToIntFromJson {
    fn execute<'a>(
        &self,
        _session: &Session,
        _signature: &FunctionSignature,
        args: &'a [Datum<'a>],
    ) -> Datum<'a> {
        // We need to try both the json::number and the json::text
        if let Some(i) = args[0]
            .as_maybe_json()
            .and_then(|j| j.get_number())
            .and_then(|d| d.to_i32())
        {
            Datum::from(i)
        } else if let Some(i) = args[0]
            .as_maybe_json()
            .and_then(|j| j.get_string())
            .and_then(|s| s.parse::<i32>().ok())
        {
            Datum::from(i)
        } else {
            Datum::Null
        }
    }
}

pub fn register_builtins(registry: &mut Registry) {
    registry.register_function(FunctionDefinition::new(
        "to_int",
        vec![DataType::Boolean],
        DataType::Integer,
        FunctionType::Scalar(&ToIntFromBoolean {}),
    ));

    registry.register_function(FunctionDefinition::new(
        "to_int",
        vec![DataType::Integer],
        DataType::Integer,
        FunctionType::Scalar(&ToIntFromInt {}),
    ));

    registry.register_function(FunctionDefinition::new(
        "to_int",
        vec![DataType::BigInt],
        DataType::Integer,
        FunctionType::Scalar(&ToIntFromBigInt {}),
    ));

    registry.register_function(FunctionDefinition::new(
        "to_int",
        vec![DataType::Decimal(0, 0)],
        DataType::Integer,
        FunctionType::Scalar(&ToIntFromDecimal {}),
    ));

    registry.register_function(FunctionDefinition::new(
        "to_int",
        vec![DataType::Text],
        DataType::Integer,
        FunctionType::Scalar(&ToIntFromText {}),
    ));

    registry.register_function(FunctionDefinition::new(
        "to_int",
        vec![DataType::Json],
        DataType::Integer,
        FunctionType::Scalar(&ToIntFromJson {}),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use data::json::OwnedJson;
    use data::rust_decimal::Decimal;

    const DUMMY_SIG: FunctionSignature = FunctionSignature {
        name: "to_int",
        args: vec![],
        ret: DataType::Integer,
    };

    #[test]
    fn test_null() {
        assert_eq!(
            ToIntFromBoolean {}.execute(&Session::new(1), &DUMMY_SIG, &[Datum::Null]),
            Datum::Null
        )
    }

    #[test]
    fn test_from_bool() {
        assert_eq!(
            ToIntFromBoolean {}.execute(&Session::new(1), &DUMMY_SIG, &[Datum::from(true)]),
            Datum::from(1)
        )
    }

    #[test]
    fn test_from_int() {
        assert_eq!(
            ToIntFromInt {}.execute(&Session::new(1), &DUMMY_SIG, &[Datum::from(1)]),
            Datum::from(1)
        )
    }

    #[test]
    fn test_from_bigint() {
        assert_eq!(
            ToIntFromBigInt {}.execute(&Session::new(1), &DUMMY_SIG, &[Datum::from(1_i64)]),
            Datum::from(1)
        )
    }

    #[test]
    fn test_from_decimal() {
        assert_eq!(
            ToIntFromDecimal {}.execute(
                &Session::new(1),
                &DUMMY_SIG,
                &[Datum::from(Decimal::new(10, 1))]
            ),
            Datum::from(1)
        )
    }

    #[test]
    fn test_from_text() {
        assert_eq!(
            ToIntFromText {}.execute(&Session::new(1), &DUMMY_SIG, &[Datum::from("1")]),
            Datum::from(1)
        )
    }

    #[test]
    fn test_from_json() {
        assert_eq!(
            ToIntFromJson {}.execute(
                &Session::new(1),
                &DUMMY_SIG,
                &[Datum::from(OwnedJson::parse("1").unwrap())]
            ),
            Datum::from(1)
        );

        assert_eq!(
            ToIntFromJson {}.execute(
                &Session::new(1),
                &DUMMY_SIG,
                &[Datum::from(OwnedJson::parse("\"1\"").unwrap())]
            ),
            Datum::from(1)
        );

        assert_eq!(
            ToIntFromJson {}.execute(
                &Session::new(1),
                &DUMMY_SIG,
                &[Datum::from(OwnedJson::parse("12345.2").unwrap())]
            ),
            Datum::from(12345)
        );
    }
}
