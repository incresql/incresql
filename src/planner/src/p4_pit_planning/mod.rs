use crate::utils::expr::{combine_predicates, decompose_predicate, move_column_references};
use crate::utils::logical::fields_for_operator;
use crate::{Field, Planner, PlannerError};
use ast::expr::*;
use ast::rel::logical::*;
use ast::rel::point_in_time;
use ast::rel::point_in_time::{Group, PointInTimeOperator};
use data::{LogicalTimestamp, Session};
use functions::registry::Registry;

pub struct PointInTimePlan {
    pub fields: Vec<Field>,
    pub operator: PointInTimeOperator,
}

impl Planner {
    /// Plan a point in time query, this optimizes the logical operator tree and then transforms into
    /// a physical plan for point in time
    pub fn plan_for_point_in_time(
        &self,
        query: LogicalOperator,
        session: &Session,
    ) -> Result<PointInTimePlan, PlannerError> {
        let (fields, operator) = self.plan_common(query, session)?;
        let operator = build_operator(operator, &self.function_registry);
        Ok(PointInTimePlan { fields, operator })
    }
}

fn build_operator(query: LogicalOperator, function_registry: &Registry) -> PointInTimeOperator {
    match query {
        LogicalOperator::Single => PointInTimeOperator::Single,
        LogicalOperator::Project(Project {
            distinct,
            expressions,
            source,
        }) => {
            assert!(!distinct, "Distinct should not be true at this point!");
            PointInTimeOperator::Project(point_in_time::Project {
                expressions: expressions.into_iter().map(|ne| ne.expression).collect(),
                source: Box::new(build_operator(*source, function_registry)),
            })
        }
        LogicalOperator::GroupBy(GroupBy {
            expressions,
            key_expressions,
            source,
        }) => {
            if key_expressions.is_empty() {
                PointInTimeOperator::SortedGroup(Group {
                    source: Box::new(build_operator(*source, function_registry)),
                    expressions: expressions.into_iter().map(|ne| ne.expression).collect(),
                    key_len: 0,
                })
            } else {
                // The key expr's have to be in the group by source.
                // We'll create a new project to do this.
                let key_len = key_expressions.len();
                let mut project_exprs = key_expressions;
                for (idx, field) in fields_for_operator(&source).enumerate() {
                    project_exprs.push(Expression::CompiledColumnReference(
                        CompiledColumnReference {
                            offset: idx,
                            datatype: field.data_type,
                        },
                    ));
                }

                let project = point_in_time::Project {
                    expressions: project_exprs,
                    source: Box::new(build_operator(*source, function_registry)),
                };

                let group_exprs = expressions
                    .into_iter()
                    .map(|mut ne| {
                        move_column_references(&mut ne.expression, key_len as isize);
                        ne.expression
                    })
                    .collect();

                PointInTimeOperator::HashGroup(Group {
                    source: Box::new(PointInTimeOperator::Project(project)),
                    expressions: group_exprs,
                    key_len,
                })
            }
        }
        LogicalOperator::Filter(Filter { predicate, source }) => {
            PointInTimeOperator::Filter(point_in_time::Filter {
                predicate,
                source: Box::new(build_operator(*source, function_registry)),
            })
        }
        LogicalOperator::Limit(Limit {
            offset,
            limit,
            source,
        }) => PointInTimeOperator::Limit(point_in_time::Limit {
            offset,
            limit,
            source: Box::new(build_operator(*source, function_registry)),
        }),
        LogicalOperator::Sort(Sort {
            sort_expressions,
            source,
        }) => PointInTimeOperator::Sort(point_in_time::Sort {
            sort_expressions,
            source: Box::new(build_operator(*source, function_registry)),
        }),
        LogicalOperator::Values(values) => {
            let data = values.data.into_iter().map(|row| {
                row.into_iter().map(|expr| {
                    if let Expression::Constant(datum, _datatype) = expr {
                        datum
                    } else {
                        panic!("Planner should have already have validated that all values exprs are constants - {:?}", expr)
                    }
                }).collect()
            }).collect();

            PointInTimeOperator::Values(point_in_time::Values {
                data,
                column_count: values.fields.len(),
            })
        }
        LogicalOperator::UnionAll(UnionAll { sources }) => {
            PointInTimeOperator::UnionAll(point_in_time::UnionAll {
                sources: sources
                    .into_iter()
                    .map(|o| build_operator(o, function_registry))
                    .collect(),
            })
        }
        LogicalOperator::ResolvedTable(ResolvedTable { columns: _, table }) => {
            PointInTimeOperator::TableScan(point_in_time::TableScan {
                table,
                // Having a timestamp in the future gives us read after write within the same ms
                // Rockdb already gives us atomic writes so I can't think of any downsides with this
                timestamp: LogicalTimestamp::MAX,
            })
        }
        LogicalOperator::TableInsert(TableInsert { table, source }) => {
            let actual_table =
                if let LogicalOperator::ResolvedTable(ResolvedTable { columns: _, table }) = *table
                {
                    table
                } else {
                    panic!("Can not insert into anything other than a resolved table")
                };

            PointInTimeOperator::TableInsert(point_in_time::TableInsert {
                table: actual_table,
                source: Box::new(build_operator(*source, function_registry)),
            })
        }
        LogicalOperator::NegateFreq(source) => {
            PointInTimeOperator::NegateFreq(Box::new(build_operator(*source, function_registry)))
        }
        LogicalOperator::TableAlias(table_alias) => {
            build_operator(*table_alias.source, function_registry)
        }
        LogicalOperator::FileScan(file_scan) => {
            PointInTimeOperator::FileScan(point_in_time::FileScan {
                directory: file_scan.directory,
                serde_options: file_scan.serde_options,
            })
        }
        LogicalOperator::Join(join) => {
            let mut non_equi = vec![];
            let mut equi_count = 0;
            for expr in decompose_predicate(join.on) {
                if let Expression::CompiledFunctionCall(function) = &expr {
                    if function.signature.name == "=" {
                        if let (
                            Expression::CompiledColumnReference(_),
                            Expression::CompiledColumnReference(_),
                        ) = (&function.args[0], &function.args[1])
                        {
                            equi_count += 1;
                            continue;
                        }
                    }
                }
                non_equi.push(expr);
            }

            PointInTimeOperator::HashJoin(point_in_time::Join {
                left: Box::new(build_operator(*join.left, function_registry)),
                right: Box::new(build_operator(*join.right, function_registry)),
                key_len: equi_count,
                non_equi_condition: combine_predicates(non_equi, function_registry),
                join_type: join.join_type,
            })
        }
        LogicalOperator::TableReference(_) => panic!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Planner, PlannerError};
    use ast::expr::{Expression, NamedExpression};
    use data::{DataType, Datum};

    #[test]
    fn test_plan_for_point_in_time() -> Result<(), PlannerError> {
        let planner = Planner::new_for_test();
        let session = Session::new(1);
        let raw_query = LogicalOperator::Project(Project {
            distinct: false,
            expressions: vec![NamedExpression {
                alias: None,
                expression: Expression::Constant(Datum::Null, DataType::Null),
            }],
            source: Box::new(LogicalOperator::Single),
        });

        let expected = PointInTimeOperator::Project(point_in_time::Project {
            expressions: vec![Expression::Constant(Datum::Null, DataType::Null)],
            source: Box::new(PointInTimeOperator::Single),
        });

        assert_eq!(
            planner
                .plan_for_point_in_time(raw_query, &session)?
                .operator,
            expected
        );
        Ok(())
    }
}
