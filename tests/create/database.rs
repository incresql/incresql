use crate::runner::*;

#[test]
fn create_databases() {
    with_connection(|connection| {
        connection.query(r#"CREATE DATABASE foobar"#, "");

        connection.query(
            r#"SELECT * FROM incresql.databases where name = "foobar""#,
            "
                |foobar|
            ",
        );

        connection.query(r#"use foobar"#, "");

        connection.query(r#"DROP DATABASE foobar"#, "");

        connection.query(
            r#"SELECT * FROM incresql.databases where name = "foobar""#,
            "",
        );
    });
}
