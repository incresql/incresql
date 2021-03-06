use crate::{Planner, PlannerError};
use ast::rel::logical::LogicalOperator;
use data::Session;
mod check_aggregates_usage;
mod check_inserts;
mod check_predicates;
mod check_unions;
mod column_aliases;
mod compile_functions_and_refs;
mod convert_project_to_groupby;
mod expand_stars;
mod resolve_tables;
mod sub_in_special_vars;
mod validate_values_types;

/// Validate the query, as part of the process of validating the query we will actually end up
/// doing all the catalog and function lookups and subbing them in.
impl Planner {
    pub fn validate(
        &self,
        mut query: LogicalOperator,
        session: &Session,
    ) -> Result<LogicalOperator, PlannerError> {
        // NOTE if adding phases before the resolve table step you will need to
        // add them inside the resolve table where it inlines any views.
        // Populate column aliases
        sub_in_special_vars::sub_in_special_vars(&mut query);
        column_aliases::normalize_column_aliases(&mut query);
        // Grab a read lock on the catalog and look up the tables
        {
            let catalog = self.catalog.read().unwrap();
            resolve_tables::resolve_tables(&catalog, &mut query, session)?;
        }
        // Now that all the fields are there we can expand all the stars
        expand_stars::expand_stars(&mut query);
        validate_values_types::validate_values_types(&mut query)?;
        compile_functions_and_refs::compile_functions(&mut query, &self.function_registry)?;

        // At this point the ast's are sane enough that we can ask expressions what types they
        // return etc.
        convert_project_to_groupby::project_to_groupby(&mut query);
        // Type checks etc
        check_aggregates_usage::check_for_aggregates(&mut query)?;
        check_predicates::check_predicates(&mut query)?;
        check_inserts::check_inserts(&mut query)?;
        check_unions::check_unions(&mut query)?;

        Ok(query)
    }
}
