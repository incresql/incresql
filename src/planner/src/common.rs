use crate::{Field, Planner, PlannerError};
use ast::expr::Expression;
use ast::rel::logical::LogicalOperator;
use data::DataType;

impl Planner {
    pub(crate) fn plan_common(
        &self,
        query: LogicalOperator,
    ) -> Result<(Vec<Field>, LogicalOperator), PlannerError> {
        let query = self.validate(query)?;
        let query = self.normalize(query)?;
        let fields = fields_for_operator(&query).collect();
        Ok((fields, query))
    }
}

/// Returns the fields for an operator, will panic if called before query is normalized
fn fields_for_operator(operator: &LogicalOperator) -> impl Iterator<Item = Field> + '_ {
    operator.named_expressions().map(|ne| Field {
        alias: ne.alias.as_ref().unwrap().clone(),
        data_type: type_for_expression(&ne.expression),
    })
}

/// Returns the datatype for an expression, will panic if called before query is normalized
pub(crate) fn type_for_expression(expr: &Expression) -> DataType {
    match expr {
        Expression::Literal(constant) => constant.datatype(),
        Expression::FunctionCall(_) => panic!(),
        Expression::Cast(cast) => cast.datatype,
        Expression::CompiledFunctionCall(function_call) => function_call.signature.ret,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::expr::NamedExpression;
    use ast::rel::logical::Project;
    use data::{rust_decimal::Decimal, Datum};
    use functions::registry::Registry;
    use std::str::FromStr;

    #[test]
    fn test_plan_common_fields() -> Result<(), PlannerError> {
        let planner = Planner::new(Registry::new(false));
        let raw_query = LogicalOperator::Project(Project {
            distinct: false,
            expressions: vec![NamedExpression {
                alias: None,
                expression: Expression::Literal(Datum::from(Decimal::from_str("1.23").unwrap())),
            }],
            source: Box::new(LogicalOperator::Single),
        });

        let (fields, _operator) = planner.plan_common(raw_query)?;

        assert_eq!(
            fields,
            vec![Field {
                alias: String::from("_col1"),
                data_type: DataType::Decimal(28, 2)
            }]
        );
        Ok(())
    }
}
