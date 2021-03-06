use crate::utils::expr::type_for_expression;
use crate::Field;
use ast::expr::{CompiledColumnReference, Expression, NamedExpression};
use ast::rel::logical::{LogicalOperator, Project};
use data::DataType;
use std::iter::{empty, once};

/// Returns the fields for an operator, will panic if called before query is normalized
pub(crate) fn fields_for_operator(
    operator: &LogicalOperator,
) -> Box<dyn Iterator<Item = Field> + '_> {
    match operator {
        LogicalOperator::Project(_) | LogicalOperator::GroupBy(_) => {
            Box::from(operator.named_expressions().map(|ne| Field {
                qualifier: None,
                alias: ne.alias.as_ref().unwrap().clone(),
                data_type: type_for_expression(&ne.expression),
            }))
        }
        LogicalOperator::Filter(filter) => fields_for_operator(&filter.source),
        LogicalOperator::Limit(limit) => fields_for_operator(&limit.source),
        LogicalOperator::Sort(sort) => fields_for_operator(&sort.source),
        LogicalOperator::Values(values) => {
            Box::from(values.fields.iter().map(|(data_type, alias)| Field {
                qualifier: None,
                alias: alias.clone(),
                data_type: *data_type,
            }))
        }
        LogicalOperator::TableAlias(table_alias) => Box::from(
            fields_for_operator(&table_alias.source).map(move |f| Field {
                qualifier: Some(table_alias.alias.clone()),
                ..f
            }),
        ),
        LogicalOperator::UnionAll(union_all) => {
            fields_for_operator(union_all.sources.first().unwrap())
        }
        LogicalOperator::ResolvedTable(table) => {
            Box::from(table.columns.iter().map(|(alias, datatype)| Field {
                qualifier: None,
                alias: alias.clone(),
                data_type: *datatype,
            }))
        }
        LogicalOperator::NegateFreq(source) => fields_for_operator(source),
        LogicalOperator::Single | LogicalOperator::TableInsert(_) => Box::from(empty()),
        LogicalOperator::FileScan(_) => Box::from(once(Field {
            qualifier: None,
            alias: "data".to_string(),
            data_type: DataType::Json,
        })),
        LogicalOperator::TableReference(_) => panic!(),
        LogicalOperator::Join(join) => {
            Box::from(fields_for_operator(&join.left).chain(fields_for_operator(&join.right)))
        }
    }
}

/// A much lighter version of fields_for_operator that can be run before function
/// and type resolution
pub(crate) fn fieldnames_for_operator(
    operator: &LogicalOperator,
) -> Box<dyn Iterator<Item = (Option<&str>, &str)> + '_> {
    match operator {
        LogicalOperator::Project(_) | LogicalOperator::GroupBy(_) => Box::from(
            operator
                .named_expressions()
                .map(|ne| (None, ne.alias.as_ref().unwrap().as_str())),
        ),
        LogicalOperator::Filter(filter) => fieldnames_for_operator(&filter.source),
        LogicalOperator::Limit(limit) => fieldnames_for_operator(&limit.source),
        LogicalOperator::Sort(sort) => fieldnames_for_operator(&sort.source),
        LogicalOperator::Values(values) => Box::from(
            values
                .fields
                .iter()
                .map(|(_datatype, alias)| (None, alias.as_str())),
        ),
        LogicalOperator::TableAlias(table_alias) => Box::from(
            fieldnames_for_operator(&table_alias.source)
                .map(move |(_, alias)| (Some(table_alias.alias.as_str()), alias)),
        ),
        LogicalOperator::UnionAll(union_all) => {
            fieldnames_for_operator(union_all.sources.first().unwrap())
        }
        LogicalOperator::ResolvedTable(table) => Box::from(
            table
                .columns
                .iter()
                .map(|(alias, _datatype)| (None, alias.as_str())),
        ),
        LogicalOperator::NegateFreq(source) => fieldnames_for_operator(source),
        LogicalOperator::FileScan(_) => Box::from(once((None, "data"))),
        LogicalOperator::Single | LogicalOperator::TableInsert(_) => Box::from(empty()),
        LogicalOperator::Join(join) => Box::from(
            fieldnames_for_operator(&join.left).chain(fieldnames_for_operator(&join.right)),
        ),
        LogicalOperator::TableReference(_) => panic!(),
    }
}

/// Returns the source fields for an operator.
/// This is the fields that expressions in the operator can "see".
/// For now this is only going to be expressions from the immediate children.
pub(crate) fn source_fields_for_operator(
    operator: &LogicalOperator,
) -> Box<dyn Iterator<Item = Field> + '_> {
    match operator {
        LogicalOperator::Project(project) => fields_for_operator(&project.source),
        LogicalOperator::GroupBy(group_by) => fields_for_operator(&group_by.source),
        LogicalOperator::Filter(filter) => fields_for_operator(&filter.source),
        LogicalOperator::Limit(limit) => fields_for_operator(&limit.source),
        LogicalOperator::Sort(sort) => fields_for_operator(&sort.source),
        LogicalOperator::TableAlias(table_alias) => fields_for_operator(&table_alias.source),
        LogicalOperator::UnionAll(union_all) => {
            fields_for_operator(union_all.sources.first().unwrap())
        }
        LogicalOperator::TableInsert(table_insert) => fields_for_operator(&table_insert.source),
        LogicalOperator::NegateFreq(source) => fields_for_operator(source),
        // The on clause see's the columns the same as the operators above do.
        LogicalOperator::Join(_) => fields_for_operator(operator),
        LogicalOperator::Values(_)
        | LogicalOperator::Single
        | LogicalOperator::TableReference(_)
        | LogicalOperator::FileScan(_)
        | LogicalOperator::ResolvedTable(_) => Box::from(empty()),
    }
}

/// Takes an operator and returns a project that wraps it.
pub(crate) fn create_wrapping_project(operator: LogicalOperator) -> Project {
    let expressions = fields_for_operator(&operator)
        .enumerate()
        .map(|(idx, field)| NamedExpression {
            alias: Some(field.alias),
            expression: Expression::CompiledColumnReference(CompiledColumnReference {
                offset: idx,
                datatype: field.data_type,
            }),
        })
        .collect();
    Project {
        distinct: false,
        expressions,
        source: Box::new(operator),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::expr::{Expression, NamedExpression};
    use ast::rel::logical::{Project, TableAlias};
    use data::rust_decimal::Decimal;
    use data::DataType;
    use std::str::FromStr;

    #[test]
    fn test_fields_for_operator() {
        let projection = LogicalOperator::Project(Project {
            distinct: false,
            expressions: vec![NamedExpression {
                alias: Some("bar".to_string()),
                expression: Expression::from(Decimal::from_str("1.23").unwrap()),
            }],
            source: Box::new(LogicalOperator::Single),
        });

        assert_eq!(
            fields_for_operator(&projection).collect::<Vec<_>>(),
            vec![Field {
                qualifier: None,
                alias: "bar".to_string(),
                data_type: DataType::Decimal(3, 2)
            }]
        );

        let table_alias = LogicalOperator::TableAlias(TableAlias {
            alias: "foo".to_string(),
            source: Box::new(projection),
        });

        assert_eq!(
            fields_for_operator(&table_alias).collect::<Vec<_>>(),
            vec![Field {
                qualifier: Some("foo".to_string()),
                alias: "bar".to_string(),
                data_type: DataType::Decimal(3, 2)
            }]
        );
    }

    #[test]
    fn test_fieldnames_for_operator() {
        let projection = LogicalOperator::Project(Project {
            distinct: false,
            expressions: vec![NamedExpression {
                alias: Some("bar".to_string()),
                expression: Expression::from(Decimal::from_str("1.23").unwrap()),
            }],
            source: Box::new(LogicalOperator::Single),
        });

        assert_eq!(
            fieldnames_for_operator(&projection).collect::<Vec<_>>(),
            vec![(None, "bar")]
        );
    }
}
