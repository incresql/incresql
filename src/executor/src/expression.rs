use crate::utils::right_size;
use ast::expr::Expression;
use data::{Datum, Session};

pub trait EvalScalar {
    /// Evaluates an expression as a scalar context
    fn eval_scalar(&self, session: &Session, row: &[Datum]) -> Datum;
}

impl EvalScalar for Expression {
    /// Evaluates a "row" of expressions as a scalar context
    #[allow(mutable_transmutes)]
    #[allow(clippy::transmute_ptr_to_ptr)]
    fn eval_scalar(&self, session: &Session, row: &[Datum]) -> Datum {
        match self {
            Expression::Literal(literal) => literal.clone(),
            // This should be compiled away by this point
            Expression::FunctionCall(_) => panic!(),
            Expression::CompiledFunctionCall(function_call) => {
                // Due to datum's being able to reference data from source datums, we need to hold
                // onto all the intermediate datums just in case. Rust lifetimes don't really allow
                // us to do this in an easy way without ref counting and allocating so hence we put
                // the buffer in the expression datastructure itself and use a little unsafe to muck
                // with the lifetimes
                let buf = unsafe {
                    std::mem::transmute::<&Vec<Datum<'_>>, &mut Vec<Datum<'_>>>(
                        &function_call.expr_buffer,
                    )
                };
                right_size(buf, &function_call.args);
                function_call.args.eval_scalar(session, row, buf);

                function_call.function.execute(session, buf)
            }
        }
    }
}

pub trait EvalScalarRow {
    fn eval_scalar<'a>(&'a self, session: &Session, source: &[Datum], target: &mut [Datum<'a>]);
}

impl EvalScalarRow for Vec<Expression> {
    fn eval_scalar<'a>(&'a self, session: &Session, source: &[Datum], target: &mut [Datum<'a>]) {
        for (idx, expr) in self.iter().enumerate() {
            target[idx] = expr.eval_scalar(session, source);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::expr::CompiledFunctionCall;
    use data::DataType;
    use functions::registry::Registry;
    use functions::FunctionSignature;

    #[test]
    fn test_eval_scalar_literal() {
        let expression = Expression::Literal(Datum::from(1234));
        let session = Session::new(1);
        assert_eq!(expression.eval_scalar(&session, &[]), Datum::from(1234));
    }

    #[test]
    fn test_eval_scalar_function() {
        let mut signature = FunctionSignature {
            name: "+",
            args: vec![DataType::Integer, DataType::Integer],
            ret: DataType::Null,
        };
        let (computed_signature, function) = Registry::new(true)
            .resolve_scalar_function(&mut signature)
            .unwrap();

        let expression = Expression::CompiledFunctionCall(CompiledFunctionCall {
            function,
            signature: Box::from(computed_signature),
            expr_buffer: vec![],
            args: vec![
                Expression::Literal(Datum::from(3)),
                Expression::Literal(Datum::from(4)),
            ],
        });

        let session = Session::new(1);
        assert_eq!(expression.eval_scalar(&session, &[]), Datum::from(7));
    }

    #[test]
    fn test_eval_scalar_row() {
        let expressions = vec![
            Expression::Literal(Datum::from(1234)),
            Expression::Literal(Datum::from(5678)),
        ];
        let session = Session::new(1);
        let mut target = vec![Datum::Null, Datum::Null];
        expressions.eval_scalar(&session, &[], &mut target);

        assert_eq!(target, vec![Datum::from(1234), Datum::from(5678)]);
    }
}
