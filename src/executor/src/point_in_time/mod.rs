use crate::point_in_time::file_scan::FileScanExecutor;
use crate::point_in_time::filter::FilterExecutor;
use crate::point_in_time::hash_group::HashGroupExecutor;
use crate::point_in_time::hash_join::HashJoinExecutor;
use crate::point_in_time::limit::LimitExecutor;
use crate::point_in_time::negate_freq::NegateFreqExecutor;
use crate::point_in_time::project::ProjectExecutor;
use crate::point_in_time::single::SingleExecutor;
use crate::point_in_time::sort::SortExecutor;
use crate::point_in_time::sorted_group::SortedGroupExecutor;
use crate::point_in_time::table_insert::TableInsertExecutor;
use crate::point_in_time::table_scan::TableScanExecutor;
use crate::point_in_time::union_all::UnionAllExecutor;
use crate::point_in_time::values::ValuesExecutor;
use crate::ExecutionError;
use ast::rel::point_in_time::PointInTimeOperator;
use data::{Session, TupleIter};
use std::sync::Arc;

mod file_scan;
mod filter;
mod hash_group;
mod hash_join;
mod limit;
mod negate_freq;
mod project;
mod single;
mod sort;
mod sorted_group;
mod table_insert;
mod table_scan;
mod union_all;
mod values;

pub type BoxedExecutor = Box<dyn TupleIter<E = ExecutionError>>;

pub fn build_executor(session: &Arc<Session>, plan: &PointInTimeOperator) -> BoxedExecutor {
    match plan {
        PointInTimeOperator::Single => Box::from(SingleExecutor::new()),
        PointInTimeOperator::Project(project) => Box::from(ProjectExecutor::new(
            Arc::clone(session),
            build_executor(session, &project.source),
            project.expressions.clone(),
        )),
        PointInTimeOperator::Filter(filter) => Box::from(FilterExecutor::new(
            Arc::clone(session),
            build_executor(session, &filter.source),
            filter.predicate.clone(),
        )),
        PointInTimeOperator::Limit(limit) => Box::from(LimitExecutor::new(
            build_executor(session, &limit.source),
            limit.offset,
            limit.limit,
        )),
        PointInTimeOperator::Sort(sort) => Box::from(SortExecutor::new(
            Arc::clone(session),
            build_executor(session, &sort.source),
            sort.sort_expressions.clone(),
        )),
        PointInTimeOperator::Values(values) => Box::from(ValuesExecutor::new(
            Box::from(values.data.clone().into_iter()),
            values.column_count,
        )),
        PointInTimeOperator::UnionAll(union_all) => Box::from(UnionAllExecutor::new(
            union_all
                .sources
                .iter()
                .map(|source| build_executor(session, source))
                .collect(),
        )),
        PointInTimeOperator::TableScan(table_scan) => Box::from(TableScanExecutor::new(
            table_scan.table.clone(),
            table_scan.timestamp,
        )),
        PointInTimeOperator::TableInsert(table_insert) => Box::from(TableInsertExecutor::new(
            build_executor(session, &table_insert.source),
            table_insert.table.clone(),
        )),
        PointInTimeOperator::NegateFreq(source) => {
            Box::from(NegateFreqExecutor::new(build_executor(session, &source)))
        }
        PointInTimeOperator::SortedGroup(group) => Box::from(SortedGroupExecutor::new(
            build_executor(session, &group.source),
            Arc::clone(&session),
            group.key_len,
            group.expressions.clone(),
        )),
        PointInTimeOperator::HashGroup(group) => Box::from(HashGroupExecutor::new(
            build_executor(session, &group.source),
            Arc::clone(&session),
            group.key_len,
            group.expressions.clone(),
        )),
        PointInTimeOperator::FileScan(file_scan) => Box::from(FileScanExecutor::new(
            file_scan.directory.clone(),
            file_scan.serde_options.clone(),
        )),
        PointInTimeOperator::HashJoin(join) => Box::from(HashJoinExecutor::new(
            build_executor(session, &join.left),
            build_executor(session, &join.right),
            join.key_len,
            join.non_equi_condition.clone(),
            join.join_type,
            Arc::clone(&session),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::expr::Expression;
    use ast::rel::point_in_time;
    use data::Datum;

    #[test]
    fn test_build_executor() -> Result<(), ExecutionError> {
        let session = Arc::new(Session::new(1));
        let plan = PointInTimeOperator::Project(point_in_time::Project {
            expressions: vec![Expression::from(1)],
            source: Box::new(PointInTimeOperator::Single),
        });

        let mut executor = build_executor(&session, &plan);
        // Due to the trait objects we can't really match against the built executor, but we can
        // run it!
        assert_eq!(executor.next()?, Some(([Datum::from(1)].as_ref(), 1)));

        assert_eq!(executor.next()?, None);
        Ok(())
    }
}
