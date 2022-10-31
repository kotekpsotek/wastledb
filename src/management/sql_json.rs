#![allow(unused)]
use serde::{ self, Serialize };
use sqlparser::{ self, ast::{ Statement, DataType, ColumnOptionDef, ColumnOption } };
use Statement::*;

/* Create table in json format */
#[derive(Serialize, Debug)]
/// Represent SQL table created from query in JSON format
pub struct JsonSQLTable {
    /// table name
    pub name: String,
    /// columns schema
    pub columns: Vec<JsonSQLTableColumn>,
    /// rows with columns. This value can be represented by None in moment when: table is now created or it doesn't have got any records inside
    pub rows: Option<Vec<Vec<JsonSQLTableColumnRow>>>
}

#[derive(Serialize, Debug)]
/// Represent all supported SQL collumn types in this database
pub enum SupportedSQLDataTypes {
    INT,
    FLOAT,
    TEXT,
    VARCHAR(Option<u16>), // can store maximum 65_535 bytes
    LONGTEXT,
    DATE,
    DATETIMESTAMP
}

#[derive(Serialize, Debug)]
#[allow(non_camel_case_types)]
/// Represent all supported column constraints in this database 
pub enum SupportedSQLColumnConstraints {
    PRIMARY_KEY,
    FOREGIN_KEY,
    DEFAULT(String),
    NOT_NULL,
    UNIQUE
}

#[derive(Serialize, Debug)]
/// Represent each column placed in JsonSQLTable
pub struct JsonSQLTableColumn {
    /// column name
    pub name: String,
    /// column data type
    pub d_type: SupportedSQLDataTypes,
    /// optional column constraints
    pub constraints: Option<Vec<SupportedSQLColumnConstraints>>
}

#[derive(Serialize, Debug)]
/// Represent each row with data for "JsonSQLTable" struct
pub struct JsonSQLTableColumnRow {
    pub col: String,
    pub value: String 
}

/// Processing attached SQL query and returns its result as "JsonSQLTable" type ready to serialize to json format thanks to "serde" and "serde_json" crates
/// When something went bad durning analyze or processing sql query then Error without any description is returned
// Note: Polish characters are not supported by sqlparser, so not use them into queries
#[must_use = "In order to assure the best level of relaibility"]
fn process_sql(sql: &str) -> Result<JsonSQLTable, ()> {
    let dialect = sqlparser::dialect::AnsiDialect {};
    let parse_and_analyze_operation = sqlparser::parser::Parser::parse_sql(&dialect, sql)
        .map_or_else(
            |err| Err(()), 
            |val| Ok(val)
        )?;

    // println!("{:?}", parse_and_analyze_operation);
    let mut processed_statements: Option<JsonSQLTable> = None;
    for statement in parse_and_analyze_operation {
        match statement {
            Statement::CreateTable { 
                or_replace: _, 
                temporary: _, 
                external: _, 
                global: _, 
                if_not_exists: _, 
                name, 
                columns: columns_row, 
                constraints: _, 
                hive_distribution: _, 
                hive_formats: _, 
                table_properties: _, 
                with_options: _, 
                file_format: _, 
                location: _, 
                query: _on1, 
                without_rowid: _, 
                like: _, 
                clone: _, 
                engine: _, 
                default_charset: _, 
                collation: _, 
                on_commit: _, 
                on_cluster: _ 
            } => {
                let table_name = &name.0[0].value;
                let mut columns: Vec<JsonSQLTableColumn> = vec![];

                    // determine columns and add its to "columns" = vector with all columns for table
                for column in columns_row {
                    let col_name = column.name.value;
                        // now are supported only 2 types: Varchar and intiger given to processed sql query
                    let col_data_type = {
                        let dt = match column.data_type {
                            DataType::Varchar(len) => {
                                // when len = None is used maximum length // specified uinit is always expressed in bytes unit
                                let r = len
                                    .clone()
                                    .map_or_else(
                                        || SupportedSQLDataTypes::VARCHAR(None), 
                                        |val| SupportedSQLDataTypes::VARCHAR(Some(len.unwrap().length as u16))
                                    );
                                Ok(r)
                            },
                            DataType::Int(_) | DataType::Integer(_) => {
                                Ok(SupportedSQLDataTypes::INT)
                            },
                            _ => Err(())
                        };
                        dt?
                    };
                        // now is supported only NOT NULL constraint
                    let col_constraint = {
                        if column.options.len() > 0 { // for convinent is "more then 0, but it only takes 1'st option so under 0 index"
                            let c_s: Result<SupportedSQLColumnConstraints, ()> = match column.options[0].clone() { // check only first option becuase only one constraint per table is now supported
                                ColumnOptionDef { name: _, option } => {
                                    match option {
                                        ColumnOption::NotNull => {
                                            Ok(SupportedSQLColumnConstraints::NOT_NULL)
                                        },
                                        _ => Err(()) // for unusported constraints
                                    }
                                },
                                _ => Err(()) // for unusported column option type
                            };
                            let c_s = c_s?;
                            Some(
                                vec![
                                    c_s
                                ]
                            ) // KEEP IN MIND: Return vector with single option (in this moment) to avoid boilerplate in section "compose column type..."
                        }
                        else {
                            None
                        }
                    };
                        // compose column type and add it to table columns collection
                    let ready_column = JsonSQLTableColumn {
                        name: col_name,
                        d_type: col_data_type,
                        constraints: col_constraint
                    };
                    columns.push(ready_column);
                };
                    // compose sql table in json format
                let json_sql_table = JsonSQLTable {
                    name: table_name.into(),
                    columns,
                    rows: None         
                };
                    // attach computed json table from sql to returned value from whole function
                processed_statements = Some(json_sql_table);
            },
            _ => continue
        }
    };
    
    // When all went good (processed statement has got initialized value) then processed statement as JsonSQLTable will be returned
    match processed_statements { // this and type Option<_> for processed_statements is required by rust safegurads system
        Some(statement) => Ok(statement),
        None => Err(())
    }
}

#[test]
fn test_process_sql() {
    let computed_table = process_sql("CREATE TABLE pieski (imie_pieska varchar(2000) NOT NULL, wiek_pieska int)").unwrap();
    println!("Computed table is:\n{:#?}", computed_table)
}
