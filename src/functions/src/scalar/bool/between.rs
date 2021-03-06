use crate::registry::Registry;
use crate::{Function, FunctionDefinition, FunctionSignature, FunctionType};
use data::{DataType, Datum, Session};

#[derive(Debug)]
struct Between {}

impl Function for Between {
    fn execute<'a>(
        &self,
        _session: &Session,
        _signature: &FunctionSignature,
        args: &'a [Datum<'a>],
    ) -> Datum<'a> {
        if args[0].is_null() || args[1].is_null() || args[2].is_null() {
            Datum::Null
        } else {
            Datum::from(args[0] >= args[1] && args[0] <= args[2])
        }
    }
}

pub fn register_builtins(registry: &mut Registry) {
    for datatype in &[
        DataType::Boolean,
        DataType::Integer,
        DataType::BigInt,
        DataType::Decimal(0, 0),
        DataType::Text,
        DataType::Date,
    ] {
        registry.register_function(FunctionDefinition::new(
            "between",
            vec![*datatype, *datatype, *datatype],
            DataType::Boolean,
            FunctionType::Scalar(&Between {}),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMMY_SIG: FunctionSignature = FunctionSignature {
        name: "between",
        args: vec![],
        ret: DataType::Boolean,
    };

    #[test]
    fn test_null() {
        assert_eq!(
            Between {}.execute(
                &Session::new(1),
                &DUMMY_SIG,
                &[Datum::Null, Datum::from(1), Datum::from(2)]
            ),
            Datum::Null
        )
    }

    #[test]
    fn test_int() {
        assert_eq!(
            Between {}.execute(
                &Session::new(1),
                &DUMMY_SIG,
                &[Datum::from(0), Datum::from(1), Datum::from(2)]
            ),
            Datum::from(false)
        );

        assert_eq!(
            Between {}.execute(
                &Session::new(1),
                &DUMMY_SIG,
                &[Datum::from(1), Datum::from(1), Datum::from(2)]
            ),
            Datum::from(true)
        );

        assert_eq!(
            Between {}.execute(
                &Session::new(1),
                &DUMMY_SIG,
                &[Datum::from(2), Datum::from(1), Datum::from(2)]
            ),
            Datum::from(true)
        );

        assert_eq!(
            Between {}.execute(
                &Session::new(1),
                &DUMMY_SIG,
                &[Datum::from(3), Datum::from(1), Datum::from(2)]
            ),
            Datum::from(false)
        );
    }
}
