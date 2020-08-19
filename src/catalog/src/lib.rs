mod bootstrap;
use data::json::JsonBuilder;
use data::{DataType, Datum, LogicalTimestamp, SortOrder, TupleIter};
use std::convert::TryFrom;
use storage::{Storage, Table};

mod error;
pub use error::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// The catalog is responsible for the lifecycles and naming of all the
/// database objects.
#[derive(Debug)]
pub struct Catalog {
    storage: Storage,
    // The lowest level of metadata stored by the catalog.
    // a table of
    // table_id:bigint(pk), column_len:int, pks_sorts:[bool]:json
    prefix_metadata_table: Table,
    // Table listing databases
    // name:text(pk)
    databases_table: Table,
    // Table listing tables
    // database_id:text(pk), table_name:text(pk), table_id:bigint, columns:json, system:bool
    tables_table: Table,
}

const PREFIX_METADATA_TABLE_ID: u32 = 0;
const DATABASES_TABLE_ID: u32 = 2;
const TABLES_TABLE_ID: u32 = 4;

impl Catalog {
    /// Creates a catalog, wrapping the passed in storage
    pub fn new(storage: Storage) -> Result<Self, CatalogError> {
        let prefix_metadata_table = storage.table(
            PREFIX_METADATA_TABLE_ID,
            vec![
                ("table_id".to_string(), DataType::BigInt),
                ("column_len".to_string(), DataType::Integer),
                ("pks_sorts".to_string(), DataType::Json),
            ],
            vec![SortOrder::Asc],
        );
        let databases_table = storage.table(
            DATABASES_TABLE_ID,
            vec![("name".to_string(), DataType::Text)],
            vec![SortOrder::Asc],
        );
        let tables_table = storage.table(
            TABLES_TABLE_ID,
            vec![
                ("database_name".to_string(), DataType::Text),
                ("name".to_string(), DataType::Text),
                ("table_id".to_string(), DataType::BigInt),
                ("columns".to_string(), DataType::Json),
                ("system".to_string(), DataType::Boolean),
            ],
            vec![SortOrder::Asc, SortOrder::Asc],
        );
        let mut catalog = Catalog {
            storage,
            prefix_metadata_table,
            databases_table,
            tables_table,
        };
        catalog.bootstrap()?;
        Ok(catalog)
    }

    /// Creates a new catalog backed by in-memory storage
    pub fn new_for_test() -> Result<Self, CatalogError> {
        Catalog::new(Storage::new_in_mem()?)
    }

    /// Returns the table with the given name
    pub fn table(&self, database: &str, table: &str) -> Result<Table, CatalogError> {
        let tables_pk = [Datum::from(database), Datum::from(table)];
        let mut key_buf = vec![];
        let mut value = vec![];

        self.tables_table
            .system_point_lookup(&tables_pk, &mut key_buf, &mut value)?
            .ok_or_else(|| CatalogError::TableNotFound(database.to_string(), table.to_string()))?;

        let id = value[0].as_bigint().unwrap() as u32;
        let columns: Vec<_> = value[1]
            .as_json()
            .unwrap()
            .iter_array()
            .unwrap()
            .map(|col| {
                let mut iter = col.iter_array().unwrap();
                let col_name = iter.next().unwrap().get_string().unwrap();
                let col_type =
                    DataType::try_from(iter.next().unwrap().get_string().unwrap()).unwrap();
                (col_name.to_string(), col_type)
            })
            .collect();

        let prefix_pk = [value[0].clone()];
        self.prefix_metadata_table
            .system_point_lookup(&prefix_pk, &mut key_buf, &mut value)?
            .unwrap();

        let pk = value[1]
            .as_json()
            .unwrap()
            .iter_array()
            .unwrap()
            .map(|b| {
                if b.get_boolean().unwrap() {
                    SortOrder::Desc
                } else {
                    SortOrder::Asc
                }
            })
            .collect();

        Ok(self.storage.table(id, columns, pk))
    }

    /// Called to create a database
    pub fn create_database(&mut self, database_name: &str) -> Result<(), CatalogError> {
        self.check_db_not_exists(database_name)?;
        self.create_database_impl(database_name)
    }

    /// Called to drop a database
    pub fn drop_database(&mut self, database_name: &str) -> Result<(), CatalogError> {
        self.check_db_exists(database_name)?;
        self.check_db_empty(database_name)?;
        // Write with freq -1
        self.databases_table.atomic_write(|batch| {
            batch.write_tuple(
                &self.databases_table,
                &[Datum::from(database_name)],
                LogicalTimestamp::now(),
                -1,
            )
        })?;
        Ok(())
    }

    /// Creates a new table
    pub fn create_table(
        &mut self,
        database_name: &str,
        table_name: &str,
        columns: &[(String, DataType)],
    ) -> Result<(), CatalogError> {
        self.check_db_exists(database_name)?;
        self.check_table_not_exists(database_name, table_name)?;
        let id = self.generate_table_id(table_name)?;
        let pk: Vec<_> = columns.iter().map(|_| SortOrder::Asc).collect();

        self.create_table_impl(database_name, table_name, id, columns, &pk, false)
    }

    /// Creates a database, doesn't do any checks to see if the database already exists etc.
    fn create_database_impl(&mut self, database_name: &str) -> Result<(), CatalogError> {
        self.databases_table.atomic_write(|batch| {
            batch.write_tuple(
                &self.databases_table,
                &[Datum::from(database_name)],
                LogicalTimestamp::now(),
                1,
            )
        })?;
        Ok(())
    }

    /// Check database empty.
    fn check_db_empty(&mut self, database_name: &str) -> Result<(), CatalogError> {
        let db_datum = [Datum::from(database_name)];
        let mut iter =
            self.tables_table
                .range_scan(Some(&db_datum), Some(&db_datum), LogicalTimestamp::MAX);
        if iter.next()?.is_some() {
            Err(CatalogError::DatabaseNotEmpty(database_name.to_string()))
        } else {
            Ok(())
        }
    }

    /// Check for if a database with that name exists
    fn check_db_exists(&mut self, database_name: &str) -> Result<(), CatalogError> {
        if !self.db_exists(database_name)? {
            Err(CatalogError::DatabaseNotFound(database_name.to_string()))
        } else {
            Ok(())
        }
    }

    fn check_db_not_exists(&mut self, database_name: &str) -> Result<(), CatalogError> {
        if self.db_exists(database_name)? {
            Err(CatalogError::DatabaseAlreadyExists(
                database_name.to_string(),
            ))
        } else {
            Ok(())
        }
    }

    fn db_exists(&mut self, database_name: &str) -> Result<bool, CatalogError> {
        let db_datum = [Datum::from(database_name)];
        let mut iter = self.databases_table.range_scan(
            Some(&db_datum),
            Some(&db_datum),
            LogicalTimestamp::MAX,
        );
        Ok(iter.next()?.is_some())
    }

    /// Check for if a database with that name exists
    fn check_table_exists(
        &mut self,
        database_name: &str,
        table_name: &str,
    ) -> Result<(), CatalogError> {
        if !self.table_exists(database_name, table_name)? {
            Err(CatalogError::TableNotFound(
                database_name.to_string(),
                table_name.to_string(),
            ))
        } else {
            Ok(())
        }
    }

    fn check_table_not_exists(
        &mut self,
        database_name: &str,
        table_name: &str,
    ) -> Result<(), CatalogError> {
        if self.table_exists(database_name, table_name)? {
            Err(CatalogError::TableAlreadyExists(
                database_name.to_string(),
                table_name.to_string(),
            ))
        } else {
            Ok(())
        }
    }

    fn table_exists(
        &mut self,
        database_name: &str,
        table_name: &str,
    ) -> Result<bool, CatalogError> {
        let table_datum = [Datum::from(database_name), Datum::from(table_name)];
        let mut iter = self.tables_table.range_scan(
            Some(&table_datum),
            Some(&table_datum),
            LogicalTimestamp::MAX,
        );
        Ok(iter.next()?.is_some())
    }

    fn generate_table_id(&mut self, table_name: &str) -> Result<u32, CatalogError> {
        let mut hasher = DefaultHasher::new();
        table_name.hash(&mut hasher);
        let mut id = hasher.finish() as u32;
        // Make sure table_id is even
        if id & 1 == 1 {
            id -= 1;
        }
        loop {
            let proposed = [Datum::from(id as i64)];
            let mut iter = self.prefix_metadata_table.range_scan(
                Some(&proposed),
                Some(&proposed),
                LogicalTimestamp::MAX,
            );
            if iter.next()?.is_none() {
                return Ok(id);
            } else {
                id += 2;
            }
        }
    }

    /// Creates a table but doesn't do any checks around the database, table, or id.
    fn create_table_impl(
        &mut self,
        database_name: &str,
        table_name: &str,
        table_id: u32,
        columns: &[(String, DataType)],
        pks: &[SortOrder],
        system: bool,
    ) -> Result<(), CatalogError> {
        let timestamp = LogicalTimestamp::now();

        let columns_datum = Datum::from(JsonBuilder::default().array(|array| {
            for (alias, datatype) in columns {
                array.push_array(|col_array| {
                    col_array.push_string(alias);
                    col_array.push_string(&format!("{:#}", datatype));
                })
            }
        }));

        let pks = Datum::from(JsonBuilder::default().array(|array| {
            for pk in pks {
                array.push_bool(pk.is_desc());
            }
        }));

        self.tables_table.atomic_write(|batch| {
            let tuple = [
                Datum::from(database_name),
                Datum::from(table_name),
                Datum::from(table_id as i64),
                columns_datum,
                Datum::from(system),
            ];
            batch.write_tuple(&self.tables_table, &tuple, timestamp, 1)?;

            let tuple = [
                Datum::from(table_id as i64),
                Datum::from(columns.len() as i32),
                pks,
            ];
            batch.write_tuple(&self.prefix_metadata_table, &tuple, timestamp, 1)
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_table() -> Result<(), CatalogError> {
        let catalog = Catalog::new_for_test()?;
        let table = catalog.table("incresql", "databases")?;

        assert_eq!(table.columns(), catalog.databases_table.columns());

        let mut iter = table.full_scan(LogicalTimestamp::MAX);
        assert_eq!(iter.next()?, Some(([Datum::from("default")].as_ref(), 1)));
        Ok(())
    }

    #[test]
    fn test_create_database() -> Result<(), CatalogError> {
        let mut catalog = Catalog::new_for_test()?;
        let dbs_table = catalog.table("incresql", "databases")?;

        catalog.create_database("abc")?;

        let mut iter = dbs_table.full_scan(LogicalTimestamp::MAX);
        assert_eq!(iter.next()?, Some(([Datum::from("abc")].as_ref(), 1)));

        assert_eq!(
            catalog.create_database("abc"),
            Err(CatalogError::DatabaseAlreadyExists("abc".to_string()))
        );

        catalog.drop_database("abc")?;
        let mut iter = dbs_table.full_scan(LogicalTimestamp::MAX);
        assert_eq!(iter.next()?, Some(([Datum::from("default")].as_ref(), 1)));
        Ok(())
    }

    #[test]
    fn test_create_table() -> Result<(), CatalogError> {
        let mut catalog = Catalog::new_for_test()?;
        let columns = vec![("a".to_string(), DataType::Integer)];

        catalog.create_table("default", "test", &columns)?;

        let table = catalog.table("default", "test")?;
        assert_eq!(
            table.columns(),
            columns.as_slice()
        );
        Ok(())
    }
}
