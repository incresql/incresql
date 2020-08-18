use crate::expr::Expression;
use data::{Datum, LogicalTimestamp};
use storage::Table;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum PointInTimeOperator {
    Single, // No from clause, ie select 1 + 1
    Project(Project),
    Values(Values),
    Filter(Filter),
    Limit(Limit),
    UnionAll(UnionAll),
    TableScan(TableScan),
}

impl Default for PointInTimeOperator {
    fn default() -> Self {
        PointInTimeOperator::Single
    }
}

/// An operator that just feeds up a fixed set of values.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Values {
    pub data: Vec<Vec<Datum<'static>>>,
    pub column_count: usize,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Project {
    pub expressions: Vec<Expression>,
    pub source: Box<PointInTimeOperator>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Filter {
    pub predicate: Expression,
    pub source: Box<PointInTimeOperator>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Limit {
    pub offset: i64,
    pub limit: i64,
    pub source: Box<PointInTimeOperator>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct UnionAll {
    pub sources: Vec<PointInTimeOperator>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct TableScan {
    pub table: Table,
    pub timestamp: LogicalTimestamp,
}
